-- Hand-authored DB guards backing the tax hook rules. The service layer
-- pre-checks these for friendly errors; these objects make the invariants hold
-- for raw SQL writers too. Protected in metaphor.codegen.yaml so
-- `schema generate` never removes it.

-- TG1: a company cannot have two live templates of the same type with the
-- same display name. Code uniqueness already exists (idx_tax_templates_company_id_code);
-- name is what users see in pickers, so ambiguity there is a support ticket.
CREATE UNIQUE INDEX IF NOT EXISTS idx_tax_templates_company_type_name
    ON tax.tax_templates (company_id, template_type, name)
    WHERE (metadata->>'deleted_at') IS NULL;

-- TG6: company_id is immutable on every tenant-owned tax row. Moving a row
-- between companies would silently strand its history and break the fence.
CREATE OR REPLACE FUNCTION tax.forbid_company_change() RETURNS trigger AS $$
BEGIN
    RAISE EXCEPTION 'company_id is immutable on %.% (row %)', TG_TABLE_SCHEMA, TG_TABLE_NAME, OLD.id
        USING ERRCODE = '23000';
END;
$$ LANGUAGE plpgsql;

DROP TRIGGER IF EXISTS tax_templates_company_immutable ON tax.tax_templates;
CREATE TRIGGER tax_templates_company_immutable
    BEFORE UPDATE OF company_id ON tax.tax_templates
    FOR EACH ROW
    WHEN (OLD.company_id IS DISTINCT FROM NEW.company_id)
    EXECUTE FUNCTION tax.forbid_company_change();

DROP TRIGGER IF EXISTS tax_template_rows_company_immutable ON tax.tax_template_rows;
CREATE TRIGGER tax_template_rows_company_immutable
    BEFORE UPDATE OF company_id ON tax.tax_template_rows
    FOR EACH ROW
    WHEN (OLD.company_id IS DISTINCT FROM NEW.company_id)
    EXECUTE FUNCTION tax.forbid_company_change();

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

DROP TRIGGER IF EXISTS company_tax_settings_company_immutable ON tax.company_tax_settings;
CREATE TRIGGER company_tax_settings_company_immutable
    BEFORE UPDATE OF company_id ON tax.company_tax_settings
    FOR EACH ROW
    WHEN (OLD.company_id IS DISTINCT FROM NEW.company_id)
    EXECUTE FUNCTION tax.forbid_company_change();

-- TG3: a cash-basis (on_payment) posture must name a transition account that
-- exists in accounting.accounts and is reconcilable — the flip machinery pairs
-- against it, so a non-reconcilable account dead-ends the deferral. When the
-- accounting schema is absent this cannot be verified at the DB level; the
-- write-path arm still refuses, so this trigger stays silent rather than
-- blocking hosts that run tax without accounting.
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

-- TG4: every repartition family (template x document_type) must be complete
-- and balanced: exactly one base line, at least one tax line, tax factors
-- summing to 100.00 at 2dp, and the invoice family present if and only if the
-- refund family is (templates are seeded with both so a document of either
-- kind always has a routing). Deferred to commit so seeding a template's
-- families line-by-line inside one transaction is legal.
CREATE OR REPLACE FUNCTION tax.validate_repartition_family() RETURNS trigger AS $$
DECLARE
    v_templates UUID[];
    v_template UUID;
    v_present INT;
    v_malformed TEXT;
BEGIN
    IF TG_OP = 'DELETE' THEN
        v_templates := ARRAY[OLD.template_id];
    ELSIF TG_OP = 'UPDATE' AND NEW.template_id IS DISTINCT FROM OLD.template_id THEN
        v_templates := ARRAY[NEW.template_id, OLD.template_id];
    ELSE
        v_templates := ARRAY[NEW.template_id];
    END IF;

    FOREACH v_template IN ARRAY v_templates LOOP
        -- how many document types have live lines for this template
        SELECT COUNT(DISTINCT r.document_type) INTO v_present
        FROM tax.tax_repartition_lines r
        WHERE r.template_id = v_template
          AND (r.metadata->>'deleted_at') IS NULL;

        IF v_present = 0 THEN
            CONTINUE;  -- template has no repartition rows at all: legacy shape, allowed
        END IF;

        IF v_present <> 2 THEN
            RAISE EXCEPTION 'repartition family incomplete for template %: invoice and refund families must be maintained together', v_template
                USING ERRCODE = '23000';
        END IF;

        SELECT string_agg(DT.document_type::text, ', ' ORDER BY DT.document_type::text) INTO v_malformed
        FROM (
            SELECT r.document_type,
                   COUNT(*) FILTER (WHERE r.repartition_type = 'base') AS base_n,
                   COUNT(*) FILTER (WHERE r.repartition_type = 'tax') AS tax_n,
                   COALESCE(SUM(r.factor_percent) FILTER (WHERE r.repartition_type = 'tax'), 0) AS tax_sum
            FROM tax.tax_repartition_lines r
            WHERE r.template_id = v_template
              AND (r.metadata->>'deleted_at') IS NULL
            GROUP BY r.document_type
        ) DT
        WHERE DT.base_n <> 1
           OR DT.tax_n < 1
           OR round(DT.tax_sum, 2) <> 100.00;

        IF v_malformed IS NOT NULL THEN
            RAISE EXCEPTION 'repartition family invalid for template % (document types: %): need exactly one base line, at least one tax line, tax factors summing to 100.00', v_template, v_malformed
                USING ERRCODE = '23000';
        END IF;
    END LOOP;

    RETURN NULL;
END;
$$ LANGUAGE plpgsql;

DROP TRIGGER IF EXISTS tax_repartition_lines_family_check ON tax.tax_repartition_lines;
CREATE CONSTRAINT TRIGGER tax_repartition_lines_family_check
    AFTER INSERT OR UPDATE OR DELETE ON tax.tax_repartition_lines
    DEFERRABLE INITIALLY DEFERRED
    FOR EACH ROW
    EXECUTE FUNCTION tax.validate_repartition_family();
