-- Down: drop the tax DB guards.
DROP TRIGGER IF EXISTS tax_repartition_lines_family_check ON tax.tax_repartition_lines;
DROP FUNCTION IF EXISTS tax.validate_repartition_family();

DROP TRIGGER IF EXISTS company_tax_settings_caba_transition_check ON tax.company_tax_settings;
DROP TRIGGER IF EXISTS tax_templates_caba_transition_check ON tax.tax_templates;
DROP FUNCTION IF EXISTS tax.check_caba_transition_reconcilable();

DROP TRIGGER IF EXISTS tax_template_rows_company_immutable ON tax.tax_template_rows;
DROP TRIGGER IF EXISTS tax_repartition_lines_company_immutable ON tax.tax_repartition_lines;
DROP TRIGGER IF EXISTS tax_tags_company_immutable ON tax.tax_tags;
DROP TRIGGER IF EXISTS company_tax_settings_company_immutable ON tax.company_tax_settings;
DROP TRIGGER IF EXISTS tax_templates_company_immutable ON tax.tax_templates;
DROP FUNCTION IF EXISTS tax.forbid_company_change();

DROP INDEX IF EXISTS idx_tax_templates_company_type_name;
