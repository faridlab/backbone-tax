-- Hand-authored (council 2026-07-03): make overlapping effective windows UNREPRESENTABLE.
-- Two rows at the same (template_id, sort_order) with overlapping [effective_from, effective_to]
-- would double-charge; two withholding rows with the same code + overlap would pick ambiguously.
-- Protected in metaphor.codegen.yaml so `schema generate` never removes it.
CREATE EXTENSION IF NOT EXISTS btree_gist;

ALTER TABLE tax.tax_template_rows
  ADD CONSTRAINT excl_tax_template_rows_no_overlap
  EXCLUDE USING gist (
    template_id WITH =,
    sort_order  WITH =,
    daterange(effective_from, COALESCE(effective_to, 'infinity'::date), '[]') WITH &&
  );

ALTER TABLE tax.withholding_categories
  ADD CONSTRAINT excl_withholding_no_overlap
  EXCLUDE USING gist (
    code WITH =,
    daterange(effective_from, COALESCE(effective_to, 'infinity'::date), '[]') WITH &&
  );
