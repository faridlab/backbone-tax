-- Down: drop enum types for tax module
DROP TYPE IF EXISTS charge_type CASCADE;
DROP TYPE IF EXISTS template_type CASCADE;
DROP TYPE IF EXISTS tax_filing_status CASCADE;
DROP TYPE IF EXISTS e_faktur_status CASCADE;
DROP TYPE IF EXISTS tax_transaction_source CASCADE;
DROP TYPE IF EXISTS tax_transaction_status CASCADE;
DROP TYPE IF EXISTS invoice_kind CASCADE;
DROP TYPE IF EXISTS tax_status CASCADE;
DROP TYPE IF EXISTS tax_kind CASCADE;
