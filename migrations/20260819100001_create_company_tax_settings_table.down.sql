-- Down: drop tax.company_tax_settings table
DROP TABLE IF EXISTS tax.company_tax_settings CASCADE;
DROP FUNCTION IF EXISTS tax.company_tax_settings_audit_timestamp() CASCADE;
