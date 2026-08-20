-- Down: drop tax.tax_repartition_lines table
DROP TABLE IF EXISTS tax.tax_repartition_lines CASCADE;
DROP FUNCTION IF EXISTS tax.tax_repartition_lines_audit_timestamp() CASCADE;
