-- Reversible best-effort: drops the objects the repair created. On a healthy
-- database this migration created nothing, so dropping is also safe (the
-- objects belong to the create migrations and re-running those restores them).
DROP TRIGGER IF EXISTS tax_tags_company_immutable ON tax.tax_tags;
DROP TRIGGER IF EXISTS tax_repartition_lines_company_immutable ON tax.tax_repartition_lines;
DROP TRIGGER IF EXISTS company_tax_settings_caba_transition_check ON tax.company_tax_settings;
DROP TRIGGER IF EXISTS tax_templates_caba_transition_check ON tax.tax_templates;
ALTER TABLE tax.tax_templates DROP CONSTRAINT IF EXISTS ck_tax_templates_caba_requires_transition;
ALTER TABLE tax.tax_templates DROP COLUMN IF EXISTS cash_basis_transition_account_id;
ALTER TABLE tax.tax_templates DROP COLUMN IF EXISTS tax_exigibility;
DROP TRIGGER IF EXISTS company_tax_settings_company_immutable ON tax.company_tax_settings;
DROP TRIGGER IF EXISTS company_tax_settings_update_audit ON tax.company_tax_settings;
DROP TRIGGER IF EXISTS company_tax_settings_insert_audit ON tax.company_tax_settings;
DROP FUNCTION IF EXISTS tax.company_tax_settings_audit_timestamp();
DROP POLICY IF EXISTS company_tax_settings_company_isolation ON tax.company_tax_settings;
DROP TABLE IF EXISTS tax.company_tax_settings;
