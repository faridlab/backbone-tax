-- Down: drop tax.tax_transactions table
DROP TABLE IF EXISTS tax.tax_transactions CASCADE;
DROP FUNCTION IF EXISTS tax.tax_transactions_audit_timestamp() CASCADE;
