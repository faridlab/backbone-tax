-- Add the cash-basis (deferred exigibility) columns to tax templates.
-- tax_exigibility resolves from the company's settings at creation and is
-- materialized on the row, so a later company posture change never rewrites
-- existing templates. An on_payment template must name its transition account;
-- the reconcilability of that account is checked by the write path and the
-- transition-account trigger in 20260819100005_tax_db_guards.

ALTER TABLE tax.tax_templates
    ADD COLUMN IF NOT EXISTS tax_exigibility tax_exigibility NOT NULL DEFAULT 'on_invoice';
ALTER TABLE tax.tax_templates
    ADD COLUMN IF NOT EXISTS cash_basis_transition_account_id UUID;

ALTER TABLE tax.tax_templates
    ADD CONSTRAINT ck_tax_templates_caba_requires_transition
    CHECK (tax_exigibility <> 'on_payment' OR cash_basis_transition_account_id IS NOT NULL);
