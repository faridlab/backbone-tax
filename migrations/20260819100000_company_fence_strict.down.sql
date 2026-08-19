-- Revert the ADR-0014 strict fence re-statement for tax module.
-- The fence predates this migration (ADR-0008-era), so the honest reverse is to
-- re-state the same live policy, not to disarm the tables: a down that disabled RLS
-- would leave company data unfenced — a posture this module never had.

-- Re-state the pre-existing fence for tax.efaktur_documents (identical policy; see header).
DROP POLICY IF EXISTS efaktur_documents_company_isolation ON tax.efaktur_documents;
CREATE POLICY efaktur_documents_company_isolation ON tax.efaktur_documents
    FOR ALL
    USING      (company_id = NULLIF(current_setting('app.company_id', true), '')::uuid)
    WITH CHECK (company_id = NULLIF(current_setting('app.company_id', true), '')::uuid);

-- Re-state the pre-existing fence for tax.tax_categories (identical policy; see header).
DROP POLICY IF EXISTS tax_categories_company_isolation ON tax.tax_categories;
CREATE POLICY tax_categories_company_isolation ON tax.tax_categories
    FOR ALL
    USING      (company_id = NULLIF(current_setting('app.company_id', true), '')::uuid)
    WITH CHECK (company_id = NULLIF(current_setting('app.company_id', true), '')::uuid);

-- Re-state the pre-existing fence for tax.tax_filing_periods (identical policy; see header).
DROP POLICY IF EXISTS tax_filing_periods_company_isolation ON tax.tax_filing_periods;
CREATE POLICY tax_filing_periods_company_isolation ON tax.tax_filing_periods
    FOR ALL
    USING      (company_id = NULLIF(current_setting('app.company_id', true), '')::uuid)
    WITH CHECK (company_id = NULLIF(current_setting('app.company_id', true), '')::uuid);

-- Re-state the pre-existing fence for tax.tax_template_rows (identical policy; see header).
DROP POLICY IF EXISTS tax_template_rows_company_isolation ON tax.tax_template_rows;
CREATE POLICY tax_template_rows_company_isolation ON tax.tax_template_rows
    FOR ALL
    USING      (company_id = NULLIF(current_setting('app.company_id', true), '')::uuid)
    WITH CHECK (company_id = NULLIF(current_setting('app.company_id', true), '')::uuid);

-- Re-state the pre-existing fence for tax.tax_templates (identical policy; see header).
DROP POLICY IF EXISTS tax_templates_company_isolation ON tax.tax_templates;
CREATE POLICY tax_templates_company_isolation ON tax.tax_templates
    FOR ALL
    USING      (company_id = NULLIF(current_setting('app.company_id', true), '')::uuid)
    WITH CHECK (company_id = NULLIF(current_setting('app.company_id', true), '')::uuid);

-- Re-state the pre-existing fence for tax.tax_transactions (identical policy; see header).
DROP POLICY IF EXISTS tax_transactions_company_isolation ON tax.tax_transactions;
CREATE POLICY tax_transactions_company_isolation ON tax.tax_transactions
    FOR ALL
    USING      (company_id = NULLIF(current_setting('app.company_id', true), '')::uuid)
    WITH CHECK (company_id = NULLIF(current_setting('app.company_id', true), '')::uuid);

-- Re-state the pre-existing fence for tax.withholding_categories (identical policy; see header).
DROP POLICY IF EXISTS withholding_categories_company_isolation ON tax.withholding_categories;
CREATE POLICY withholding_categories_company_isolation ON tax.withholding_categories
    FOR ALL
    USING      (company_id = NULLIF(current_setting('app.company_id', true), '')::uuid)
    WITH CHECK (company_id = NULLIF(current_setting('app.company_id', true), '')::uuid);

