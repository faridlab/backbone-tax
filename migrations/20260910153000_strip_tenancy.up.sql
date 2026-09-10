-- Hand-authored (user-owned). Not regenerated.
--
-- Strip every company-fence artifact from the tax tables (ADR-0029): the module is
-- tenant-agnostic; org scoping is installed by the COMPOSING service's tenancy decorator,
-- never by the module. Dropped here, per table: the company-leading indexes, the
-- <table>_company_isolation RLS policy, and the company_id column itself — plus the
-- company-immutable guard triggers and the tax.forbid_company_change() function they run,
-- which have no target once the column is gone.
--
-- Ordering guard (the decorator must run FIRST on any database with data): the module
-- never moves tenancy data. A table is safe to strip when EITHER
--   a) it carries org_unit_id with no NULLs — the decorator backfilled it from company_id —
--      or b) it is empty (a fresh database: the earlier chain files created it empty).
-- Otherwise the strip RAISEs, naming the decorator step, rather than dropping a column
-- that still holds the only tenancy key. The file is re-runnable (every drop is IF EXISTS
-- and the tracker has no checksums), so a failed run retries cleanly after the decorator
-- lands.
--
-- RLS enable/force flags are deliberately NOT touched: the decorator owns those now.
--
-- Posture uniques are dropped and intentionally NOT restored tenant-free — the pre-strip
-- forms key on company_id, and a tenant-free variant would falsely collide two units' rows:
--   per-unit code uniques on tax_categories / tax_templates / tax_tags, the per-unit
--   (template_type, name) unique, e-Faktur's (unit, period, sequence) density and per-unit
--   number uniques, the one-filing-period-per-unit-per-month unique, and the one-settings-
--   row-per-unit unique on company_tax_settings. The composing service's tenancy decorator
--   installs the per-unit forms at composition time.
--
-- Two DOMAIN constraints ARE restored tenant-free, because their subjects are not
-- tenancy-scoped:
--   the tax-transactions idempotency unique re-bases to (invoice_ref, invoice_kind) — an
--   invoice id names exactly one unit's record — and the withholding overlap EXCLUDE
--   returns to its original pre-tenant shape (code + effective window), with its name
--   preserved across the reshape, as the tenant-scope migration itself did.

DO $$
DECLARE
    t text;
    has_org boolean;
    org_nulls bigint;
    total bigint;
    offenders text := '';
BEGIN
    FOREACH t IN ARRAY ARRAY[
        'tax_categories', 'tax_templates', 'tax_template_rows', 'withholding_categories',
        'company_tax_settings', 'tax_tags', 'tax_repartition_lines', 'tax_transactions',
        'efaktur_documents', 'tax_filing_periods'
    ]
    LOOP
        IF to_regclass(format('tax.%I', t)) IS NULL THEN
            CONTINUE; -- chain not fully applied on this database; nothing to strip
        END IF;

        SELECT EXISTS (
                   SELECT 1 FROM information_schema.columns
                   WHERE table_schema = 'tax' AND table_name = t AND column_name = 'org_unit_id'
               )
        INTO has_org;

        EXECUTE format('SELECT count(*) FROM tax.%I', t) INTO total;

        IF has_org THEN
            EXECUTE format(
                'SELECT count(*) FROM tax.%I WHERE org_unit_id IS NULL', t)
            INTO org_nulls;
        ELSE
            org_nulls := total; -- no org column: every row's only tenancy key is company_id
        END IF;

        IF has_org AND org_nulls = 0 THEN
            CONTINUE; -- decorator backfilled: safe
        END IF;
        IF total = 0 THEN
            CONTINUE; -- empty table (fresh database): safe
        END IF;
        offenders := offenders || format(' tax.%s (%s rows, %s rows not covered by org_unit_id);', t, total, org_nulls);
    END LOOP;

    IF offenders <> '' THEN
        RAISE EXCEPTION 'refusing to strip company_id — these tables are not yet covered by the tenancy decorator:%. Apply the composing service''s tenancy decorator (it backfills org_unit_id from company_id) and re-run; it is the only step that moves tenancy data.', offenders;
    END IF;
END $$;

-- ── company-immutable guard triggers (no target once the column is gone) ───────
DROP TRIGGER IF EXISTS tax_templates_company_immutable ON tax.tax_templates;
DROP TRIGGER IF EXISTS tax_template_rows_company_immutable ON tax.tax_template_rows;
DROP TRIGGER IF EXISTS tax_repartition_lines_company_immutable ON tax.tax_repartition_lines;
DROP TRIGGER IF EXISTS tax_tags_company_immutable ON tax.tax_tags;
DROP TRIGGER IF EXISTS company_tax_settings_company_immutable ON tax.company_tax_settings;
DROP FUNCTION IF EXISTS tax.forbid_company_change();

-- ── tax_categories ─────────────────────────────────────────────────────────────
DROP INDEX IF EXISTS tax.idx_tax_categories_company_id_code;
DROP INDEX IF EXISTS tax.idx_tax_categories_company_id;
DROP POLICY IF EXISTS tax_categories_company_isolation ON tax.tax_categories;
ALTER TABLE tax.tax_categories DROP COLUMN IF EXISTS company_id;

-- ── tax_templates ──────────────────────────────────────────────────────────────
DROP INDEX IF EXISTS tax.idx_tax_templates_company_id_code;
DROP INDEX IF EXISTS tax.idx_tax_templates_company_id;
DROP INDEX IF EXISTS tax.idx_tax_templates_company_type_name;
DROP POLICY IF EXISTS tax_templates_company_isolation ON tax.tax_templates;
ALTER TABLE tax.tax_templates DROP COLUMN IF EXISTS company_id;

-- ── tax_template_rows ──────────────────────────────────────────────────────────
DROP INDEX IF EXISTS tax.idx_tax_template_rows_company_id;
DROP POLICY IF EXISTS tax_template_rows_company_isolation ON tax.tax_template_rows;
ALTER TABLE tax.tax_template_rows DROP COLUMN IF EXISTS company_id;

-- ── withholding_categories ─────────────────────────────────────────────────────
DROP INDEX IF EXISTS tax.idx_withholding_categories_company_id;
DROP POLICY IF EXISTS withholding_categories_company_isolation ON tax.withholding_categories;
-- The overlap EXCLUDE is a DOMAIN invariant, not posture: re-create it in the module's
-- original pre-tenant shape (name preserved, same maneuver the tenant-scope migration used).
ALTER TABLE tax.withholding_categories DROP CONSTRAINT IF EXISTS excl_withholding_no_overlap;
ALTER TABLE tax.withholding_categories
  ADD CONSTRAINT excl_withholding_no_overlap
  EXCLUDE USING gist (
    code WITH =,
    daterange(effective_from, COALESCE(effective_to, 'infinity'::date), '[]') WITH &&
  );
ALTER TABLE tax.withholding_categories DROP COLUMN IF EXISTS company_id;

-- ── company_tax_settings ───────────────────────────────────────────────────────
DROP INDEX IF EXISTS tax.idx_company_tax_settings_company_id;
DROP POLICY IF EXISTS company_tax_settings_company_isolation ON tax.company_tax_settings;
ALTER TABLE tax.company_tax_settings DROP COLUMN IF EXISTS company_id;

-- ── tax_tags ───────────────────────────────────────────────────────────────────
DROP INDEX IF EXISTS tax.idx_tax_tags_company_id_code;
DROP POLICY IF EXISTS tax_tags_company_isolation ON tax.tax_tags;
ALTER TABLE tax.tax_tags DROP COLUMN IF EXISTS company_id;

-- ── tax_repartition_lines ──────────────────────────────────────────────────────
DROP POLICY IF EXISTS tax_repartition_lines_company_isolation ON tax.tax_repartition_lines;
ALTER TABLE tax.tax_repartition_lines DROP COLUMN IF EXISTS company_id;

-- ── tax_transactions ───────────────────────────────────────────────────────────
DROP INDEX IF EXISTS tax.idx_tax_transactions_company_id_invoice_ref_invoice_kind;
DROP INDEX IF EXISTS tax.idx_tax_transactions_company_id_posting_date;
DROP POLICY IF EXISTS tax_transactions_company_isolation ON tax.tax_transactions;
ALTER TABLE tax.tax_transactions DROP COLUMN IF EXISTS company_id;

-- ── efaktur_documents ──────────────────────────────────────────────────────────
DROP INDEX IF EXISTS tax.idx_efaktur_documents_company_id_period_sequence;
DROP INDEX IF EXISTS tax.idx_efaktur_documents_company_id_number;
DROP POLICY IF EXISTS efaktur_documents_company_isolation ON tax.efaktur_documents;
ALTER TABLE tax.efaktur_documents DROP COLUMN IF EXISTS company_id;

-- ── tax_filing_periods ─────────────────────────────────────────────────────────
DROP INDEX IF EXISTS tax.idx_tax_filing_periods_company_id_period;
DROP POLICY IF EXISTS tax_filing_periods_company_isolation ON tax.tax_filing_periods;
ALTER TABLE tax.tax_filing_periods DROP COLUMN IF EXISTS company_id;

-- ── Restore the tenant-free domain indexes ─────────────────────────────────────
-- The idempotency unique is IDENTITY, not posture: an invoice id names exactly one
-- unit's record, so the same predicate survives on the tenant-free column pair. The
-- posting-date scan is a DOMAIN read (the masa-pajak rollup aggregates by posting
-- window), re-based onto its tenant-free column. Everything else the dropped
-- company-leading indexes served is POSTURE and stays with the decorator (see the
-- header note).
CREATE UNIQUE INDEX IF NOT EXISTS idx_tax_transactions_invoice_ref_invoice_kind
    ON tax.tax_transactions (invoice_ref, invoice_kind) WHERE (metadata->>'deleted_at') IS NULL;
CREATE INDEX IF NOT EXISTS idx_tax_transactions_posting_date
    ON tax.tax_transactions (posting_date);
