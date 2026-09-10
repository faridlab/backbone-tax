-- Hand-authored (user-owned). Not regenerated.
--
-- Best-effort restore sketch for the tenancy strip (ADR-0029). This is a breaking module
-- release against dev-stage databases: the down re-adds the company_id column as nullable
-- with the company-leading indexes in their original shapes, but restores NO data —
-- rows written after the strip (or after the decorator re-keyed them) carry org_unit_id
-- only. The composing service's tenancy decorator remains the live fence; treat this
-- down as a schema-shape sketch for archaeology, not a usable rollback.

ALTER TABLE tax.tax_categories        ADD COLUMN IF NOT EXISTS company_id uuid;
ALTER TABLE tax.tax_templates         ADD COLUMN IF NOT EXISTS company_id uuid;
ALTER TABLE tax.tax_template_rows     ADD COLUMN IF NOT EXISTS company_id uuid;
ALTER TABLE tax.withholding_categories ADD COLUMN IF NOT EXISTS company_id uuid;
ALTER TABLE tax.company_tax_settings  ADD COLUMN IF NOT EXISTS company_id uuid;
ALTER TABLE tax.tax_tags              ADD COLUMN IF NOT EXISTS company_id uuid;
ALTER TABLE tax.tax_repartition_lines ADD COLUMN IF NOT EXISTS company_id uuid;
ALTER TABLE tax.tax_transactions      ADD COLUMN IF NOT EXISTS company_id uuid;
ALTER TABLE tax.efaktur_documents     ADD COLUMN IF NOT EXISTS company_id uuid;
ALTER TABLE tax.tax_filing_periods    ADD COLUMN IF NOT EXISTS company_id uuid;

-- The strip's restored tenant-free domain indexes/constraint go away again (the
-- company-leading variants would need company data this sketch does not restore).
DROP INDEX IF EXISTS tax.idx_tax_transactions_invoice_ref_invoice_kind;
DROP INDEX IF EXISTS tax.idx_tax_transactions_posting_date;
ALTER TABLE tax.withholding_categories DROP CONSTRAINT IF EXISTS excl_withholding_no_overlap;

CREATE INDEX IF NOT EXISTS idx_tax_categories_company_id_code
    ON tax.tax_categories (company_id, code) WHERE (metadata->>'deleted_at') IS NULL;
CREATE INDEX IF NOT EXISTS idx_tax_categories_company_id
    ON tax.tax_categories (company_id);
CREATE UNIQUE INDEX IF NOT EXISTS idx_tax_templates_company_id_code
    ON tax.tax_templates (company_id, code) WHERE (metadata->>'deleted_at') IS NULL;
CREATE INDEX IF NOT EXISTS idx_tax_templates_company_id
    ON tax.tax_templates (company_id);
CREATE UNIQUE INDEX IF NOT EXISTS idx_tax_templates_company_type_name
    ON tax.tax_templates (company_id, template_type, name) WHERE (metadata->>'deleted_at') IS NULL;
CREATE INDEX IF NOT EXISTS idx_tax_template_rows_company_id
    ON tax.tax_template_rows (company_id);
CREATE INDEX IF NOT EXISTS idx_withholding_categories_company_id
    ON tax.withholding_categories (company_id);
ALTER TABLE tax.withholding_categories
  ADD CONSTRAINT excl_withholding_no_overlap
  EXCLUDE USING gist (
    company_id WITH =,
    code WITH =,
    daterange(effective_from, COALESCE(effective_to, 'infinity'::date), '[]') WITH &&
  );
CREATE UNIQUE INDEX IF NOT EXISTS idx_company_tax_settings_company_id
    ON tax.company_tax_settings (company_id) WHERE (metadata->>'deleted_at') IS NULL;
CREATE UNIQUE INDEX IF NOT EXISTS idx_tax_tags_company_id_code
    ON tax.tax_tags (company_id, code) WHERE (metadata->>'deleted_at') IS NULL;
CREATE UNIQUE INDEX IF NOT EXISTS idx_tax_transactions_company_id_invoice_ref_invoice_kind
    ON tax.tax_transactions (company_id, invoice_ref, invoice_kind) WHERE (metadata->>'deleted_at') IS NULL;
CREATE INDEX IF NOT EXISTS idx_tax_transactions_company_id_posting_date
    ON tax.tax_transactions (company_id, posting_date);
CREATE UNIQUE INDEX IF NOT EXISTS idx_efaktur_documents_company_id_period_sequence
    ON tax.efaktur_documents (company_id, period, sequence) WHERE (metadata->>'deleted_at') IS NULL;
CREATE UNIQUE INDEX IF NOT EXISTS idx_efaktur_documents_company_id_number
    ON tax.efaktur_documents (company_id, number) WHERE (metadata->>'deleted_at') IS NULL;
CREATE UNIQUE INDEX IF NOT EXISTS idx_tax_filing_periods_company_id_period
    ON tax.tax_filing_periods (company_id, period) WHERE (metadata->>'deleted_at') IS NULL;

-- Not restored (see the up file's header): the RLS policies, the company-immutable
-- guard triggers + tax.forbid_company_change(), and the FORCE flags — the composing
-- service's tenancy decorator owns the fence now.
