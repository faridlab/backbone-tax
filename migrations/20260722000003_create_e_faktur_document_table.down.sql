-- Down: drop tax.efaktur_documents table
DROP TABLE IF EXISTS tax.efaktur_documents CASCADE;
DROP FUNCTION IF EXISTS tax.efaktur_documents_audit_timestamp() CASCADE;
