-- Repair for databases where the earlier enum-creation guards were schema-blind.
--
-- The original DO-blocks checked `pg_type.typname` bare, and pg_type spans every
-- schema in the database: in a shared database where a sibling module had already
-- created a same-named enum (billing.tax_exigibility) BEFORE these migrations ran,
-- the check wrongly concluded the type existed and skipped creating it. Every
-- statement referencing the type then failed — while a migration runner without
-- ON_ERROR_STOP recorded the migration as applied. The exposed gap, in arrival
-- order: the public tax_exigibility enum, the whole tax.company_tax_settings table
-- (indexes, CHECK, RLS fence, policy, audit triggers), the tax_templates
-- tax_exigibility column + its CHECK, and the cash-basis transition triggers.
--
-- This file re-asserts every enum-dependent object idempotently, so it converges a
-- half-applied database and is a pure no-op on a healthy one. The guard bug itself
-- is fixed in place in the create migrations (namespace-scoped checks), so fresh
-- databases never need this repair.

-- ── Enums ────────────────────────────────────────────────────────────────────

DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_type t JOIN pg_namespace n ON n.oid = t.typnamespace
        WHERE t.typname = 'tax_rounding_method' AND n.nspname = 'public'
    ) THEN
        CREATE TYPE public.tax_rounding_method AS ENUM ('round_globally', 'round_per_line');
    END IF;
END
$$;

DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_type t JOIN pg_namespace n ON n.oid = t.typnamespace
        WHERE t.typname = 'tax_exigibility' AND n.nspname = 'public'
    ) THEN
        CREATE TYPE public.tax_exigibility AS ENUM ('on_invoice', 'on_payment');
    END IF;
END
$$;

DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_type t JOIN pg_namespace n ON n.oid = t.typnamespace
        WHERE t.typname = 'repartition_document_type' AND n.nspname = 'public'
    ) THEN
        CREATE TYPE public.repartition_document_type AS ENUM ('invoice', 'refund');
    END IF;
END
$$;

DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_type t JOIN pg_namespace n ON n.oid = t.typnamespace
        WHERE t.typname = 'repartition_type' AND n.nspname = 'public'
    ) THEN
        CREATE TYPE public.repartition_type AS ENUM ('base', 'tax');
    END IF;
END
$$;

-- ── company_tax_settings (everything the failed CREATE TABLE stranded) ───────

CREATE SCHEMA IF NOT EXISTS tax;

CREATE TABLE IF NOT EXISTS tax.company_tax_settings (
    id UUID NOT NULL DEFAULT gen_random_uuid(),
    company_id UUID NOT NULL,
    rounding_method tax_rounding_method NOT NULL DEFAULT 'round_globally',
    default_exigibility tax_exigibility NOT NULL DEFAULT 'on_invoice',
    cash_basis_transition_account_id UUID,
    metadata JSONB NOT NULL DEFAULT '{"created_at":null,"updated_at":null,"deleted_at":null,"created_by":null,"updated_by":null,"deleted_by":null}'::jsonb,
    PRIMARY KEY (id)
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_company_tax_settings_company_id
    ON tax.company_tax_settings (company_id) WHERE (metadata->>'deleted_at') IS NULL;

-- A company defaulting to cash-basis exigibility must name its transition account.
DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint
        WHERE conname = 'ck_company_tax_settings_caba_requires_transition'
          AND conrelid = 'tax.company_tax_settings'::regclass
    ) THEN
        ALTER TABLE tax.company_tax_settings
            ADD CONSTRAINT ck_company_tax_settings_caba_requires_transition
            CHECK (default_exigibility <> 'on_payment' OR cash_basis_transition_account_id IS NOT NULL);
    END IF;
END
$$;

-- Company fence (ADR-0014 strict) — restated per the create migration.
ALTER TABLE tax.company_tax_settings ENABLE ROW LEVEL SECURITY;
ALTER TABLE tax.company_tax_settings FORCE  ROW LEVEL SECURITY;
DROP POLICY IF EXISTS company_tax_settings_company_isolation ON tax.company_tax_settings;
CREATE POLICY company_tax_settings_company_isolation ON tax.company_tax_settings
    FOR ALL
    USING      (company_id = NULLIF(current_setting('app.company_id', true), '')::uuid)
    WITH CHECK (company_id = NULLIF(current_setting('app.company_id', true), '')::uuid);

CREATE INDEX IF NOT EXISTS idx_company_tax_settings_metadata_gin
    ON tax.company_tax_settings USING GIN (metadata);
CREATE INDEX IF NOT EXISTS idx_company_tax_settings_metadata_deleted_at
    ON tax.company_tax_settings ((metadata->>'deleted_at'));
CREATE INDEX IF NOT EXISTS idx_company_tax_settings_metadata_created_at
    ON tax.company_tax_settings ((metadata->>'created_at'));
CREATE INDEX IF NOT EXISTS idx_company_tax_settings_metadata_updated_at
    ON tax.company_tax_settings ((metadata->>'updated_at'));

CREATE OR REPLACE FUNCTION tax.company_tax_settings_audit_timestamp() RETURNS trigger AS $$
BEGIN
    IF TG_OP = 'INSERT' THEN
        NEW.metadata = jsonb_set(NEW.metadata::jsonb, '{created_at}', to_jsonb(NOW()));
        NEW.metadata = jsonb_set(NEW.metadata::jsonb, '{updated_at}', to_jsonb(NOW()));
    ELSIF TG_OP = 'UPDATE' THEN
        NEW.metadata = jsonb_set(NEW.metadata::jsonb, '{updated_at}', to_jsonb(NOW()));
    END IF;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

DROP TRIGGER IF EXISTS company_tax_settings_insert_audit ON tax.company_tax_settings;
CREATE TRIGGER company_tax_settings_insert_audit BEFORE INSERT ON tax.company_tax_settings
    FOR EACH ROW EXECUTE FUNCTION tax.company_tax_settings_audit_timestamp();

DROP TRIGGER IF EXISTS company_tax_settings_update_audit ON tax.company_tax_settings;
CREATE TRIGGER company_tax_settings_update_audit BEFORE UPDATE ON tax.company_tax_settings
    FOR EACH ROW EXECUTE FUNCTION tax.company_tax_settings_audit_timestamp();

-- ── company immutability + cash-basis transition guards (TG6 / TG3) ──────────

CREATE OR REPLACE FUNCTION tax.forbid_company_change() RETURNS trigger AS $$
BEGIN
    RAISE EXCEPTION 'company_id is immutable on %.% (row %)', TG_TABLE_SCHEMA, TG_TABLE_NAME, OLD.id
        USING ERRCODE = '23000';
END;
$$ LANGUAGE plpgsql;

DROP TRIGGER IF EXISTS company_tax_settings_company_immutable ON tax.company_tax_settings;
CREATE TRIGGER company_tax_settings_company_immutable
    BEFORE UPDATE OF company_id ON tax.company_tax_settings
    FOR EACH ROW
    WHEN (OLD.company_id IS DISTINCT FROM NEW.company_id)
    EXECUTE FUNCTION tax.forbid_company_change();

CREATE OR REPLACE FUNCTION tax.check_caba_transition_reconcilable() RETURNS trigger AS $$
DECLARE
    v_account UUID;
    v_reconcilable BOOLEAN;
BEGIN
    IF to_regclass('accounting.accounts') IS NULL THEN
        RETURN NEW;
    END IF;

    IF TG_TABLE_NAME = 'tax_templates' THEN
        IF NEW.tax_exigibility = 'on_payment' THEN
            v_account := NEW.cash_basis_transition_account_id;
        END IF;
    ELSE
        IF NEW.default_exigibility = 'on_payment' THEN
            v_account := NEW.cash_basis_transition_account_id;
        END IF;
    END IF;

    IF v_account IS NULL THEN
        RETURN NEW;
    END IF;

    SELECT a.is_reconcilable INTO v_reconcilable
    FROM accounting.accounts a
    WHERE a.id = v_account;

    IF v_reconcilable IS NOT TRUE THEN
        RAISE EXCEPTION 'cash-basis transition account % on %.% is missing or not reconcilable',
            v_account, TG_TABLE_SCHEMA, TG_TABLE_NAME
            USING ERRCODE = '23000';
    END IF;

    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

-- ── tax_templates cash-basis columns + guards (the failed half of the ALTERs) ─

ALTER TABLE tax.tax_templates
    ADD COLUMN IF NOT EXISTS tax_exigibility tax_exigibility NOT NULL DEFAULT 'on_invoice';
ALTER TABLE tax.tax_templates
    ADD COLUMN IF NOT EXISTS cash_basis_transition_account_id UUID;

DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint
        WHERE conname = 'ck_tax_templates_caba_requires_transition'
          AND conrelid = 'tax.tax_templates'::regclass
    ) THEN
        ALTER TABLE tax.tax_templates
            ADD CONSTRAINT ck_tax_templates_caba_requires_transition
            CHECK (tax_exigibility <> 'on_payment' OR cash_basis_transition_account_id IS NOT NULL);
    END IF;
END
$$;

DROP TRIGGER IF EXISTS tax_templates_caba_transition_check ON tax.tax_templates;
CREATE TRIGGER tax_templates_caba_transition_check
    BEFORE INSERT OR UPDATE OF tax_exigibility, cash_basis_transition_account_id
    ON tax.tax_templates
    FOR EACH ROW
    EXECUTE FUNCTION tax.check_caba_transition_reconcilable();

DROP TRIGGER IF EXISTS company_tax_settings_caba_transition_check ON tax.company_tax_settings;
CREATE TRIGGER company_tax_settings_caba_transition_check
    BEFORE INSERT OR UPDATE OF default_exigibility, cash_basis_transition_account_id
    ON tax.company_tax_settings
    FOR EACH ROW
    EXECUTE FUNCTION tax.check_caba_transition_reconcilable();

-- Repartition-line guards from the DB-guards migration that reference the
-- repaired tables — re-asserted so the same convergence covers them.
DROP TRIGGER IF EXISTS tax_repartition_lines_company_immutable ON tax.tax_repartition_lines;
CREATE TRIGGER tax_repartition_lines_company_immutable
    BEFORE UPDATE OF company_id ON tax.tax_repartition_lines
    FOR EACH ROW
    WHEN (OLD.company_id IS DISTINCT FROM NEW.company_id)
    EXECUTE FUNCTION tax.forbid_company_change();

DROP TRIGGER IF EXISTS tax_tags_company_immutable ON tax.tax_tags;
CREATE TRIGGER tax_tags_company_immutable
    BEFORE UPDATE OF company_id ON tax.tax_tags
    FOR EACH ROW
    WHEN (OLD.company_id IS DISTINCT FROM NEW.company_id)
    EXECUTE FUNCTION tax.forbid_company_change();
