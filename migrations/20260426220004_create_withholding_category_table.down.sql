-- Down: drop tax.withholding_categories table
DROP TABLE IF EXISTS tax.withholding_categories CASCADE;
DROP FUNCTION IF EXISTS tax.withholding_categories_audit_timestamp() CASCADE;
