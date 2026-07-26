-- Down: remove the company RLS fence for tax module

-- Reverse the company RLS fence for tax.tax_categories
DROP POLICY IF EXISTS tax_categories_company_isolation ON tax.tax_categories;
ALTER TABLE tax.tax_categories NO FORCE ROW LEVEL SECURITY;
ALTER TABLE tax.tax_categories DISABLE ROW LEVEL SECURITY;

-- Reverse the company RLS fence for tax.tax_transactions
DROP POLICY IF EXISTS tax_transactions_company_isolation ON tax.tax_transactions;
ALTER TABLE tax.tax_transactions NO FORCE ROW LEVEL SECURITY;
ALTER TABLE tax.tax_transactions DISABLE ROW LEVEL SECURITY;

-- Reverse the company RLS fence for tax.efaktur_documents
DROP POLICY IF EXISTS efaktur_documents_company_isolation ON tax.efaktur_documents;
ALTER TABLE tax.efaktur_documents NO FORCE ROW LEVEL SECURITY;
ALTER TABLE tax.efaktur_documents DISABLE ROW LEVEL SECURITY;

-- Reverse the company RLS fence for tax.tax_filing_periods
DROP POLICY IF EXISTS tax_filing_periods_company_isolation ON tax.tax_filing_periods;
ALTER TABLE tax.tax_filing_periods NO FORCE ROW LEVEL SECURITY;
ALTER TABLE tax.tax_filing_periods DISABLE ROW LEVEL SECURITY;

-- Reverse the company RLS fence for tax.tax_templates
DROP POLICY IF EXISTS tax_templates_company_isolation ON tax.tax_templates;
ALTER TABLE tax.tax_templates NO FORCE ROW LEVEL SECURITY;
ALTER TABLE tax.tax_templates DISABLE ROW LEVEL SECURITY;

-- Reverse the company RLS fence for tax.tax_template_rows
DROP POLICY IF EXISTS tax_template_rows_company_isolation ON tax.tax_template_rows;
ALTER TABLE tax.tax_template_rows NO FORCE ROW LEVEL SECURITY;
ALTER TABLE tax.tax_template_rows DISABLE ROW LEVEL SECURITY;

-- Reverse the company RLS fence for tax.withholding_categories
DROP POLICY IF EXISTS withholding_categories_company_isolation ON tax.withholding_categories;
ALTER TABLE tax.withholding_categories NO FORCE ROW LEVEL SECURITY;
ALTER TABLE tax.withholding_categories DISABLE ROW LEVEL SECURITY;

