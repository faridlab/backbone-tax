-- Down: drop tax.tax_filing_periods table
DROP TABLE IF EXISTS tax.tax_filing_periods CASCADE;
DROP FUNCTION IF EXISTS tax.tax_filing_periods_audit_timestamp() CASCADE;
