-- Down: drop tax.tax_template_rows table
DROP TABLE IF EXISTS tax.tax_template_rows CASCADE;
DROP FUNCTION IF EXISTS tax.tax_template_rows_audit_timestamp() CASCADE;
