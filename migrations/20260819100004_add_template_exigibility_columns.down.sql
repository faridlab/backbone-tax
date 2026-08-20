-- Down: remove the cash-basis columns from tax templates.
ALTER TABLE tax.tax_templates DROP CONSTRAINT IF EXISTS ck_tax_templates_caba_requires_transition;
ALTER TABLE tax.tax_templates DROP COLUMN IF EXISTS cash_basis_transition_account_id;
ALTER TABLE tax.tax_templates DROP COLUMN IF EXISTS tax_exigibility;
