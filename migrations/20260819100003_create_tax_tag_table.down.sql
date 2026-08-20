-- Down: drop tax.tax_tags table
DROP TABLE IF EXISTS tax.tax_tags CASCADE;
DROP FUNCTION IF EXISTS tax.tax_tags_audit_timestamp() CASCADE;
