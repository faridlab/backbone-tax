-- Down: drop the declared foreign keys for tax module
ALTER TABLE IF EXISTS tax.tax_template_rows DROP CONSTRAINT IF EXISTS fk_tax_template_rows_template_id;
ALTER TABLE IF EXISTS tax.tax_templates DROP CONSTRAINT IF EXISTS fk_tax_templates_tax_category_id;
ALTER TABLE IF EXISTS tax.tax_repartition_lines DROP CONSTRAINT IF EXISTS fk_tax_repartition_lines_template_id;
ALTER TABLE IF EXISTS tax.efaktur_documents DROP CONSTRAINT IF EXISTS fk_efaktur_documents_tax_transaction_id;
