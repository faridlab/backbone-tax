-- Down: drop tax.tax_templates table
DROP TABLE IF EXISTS tax.tax_templates CASCADE;
DROP FUNCTION IF EXISTS tax.tax_templates_audit_timestamp() CASCADE;
