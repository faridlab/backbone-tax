-- Down: drop tax.tax_categories table
DROP TABLE IF EXISTS tax.tax_categories CASCADE;
DROP FUNCTION IF EXISTS tax.tax_categories_audit_timestamp() CASCADE;
