# ADR-001: The tax engine bounded context (engine now, rules deferred)

**Status**: Accepted — **Applied 2026-07-03**
**Deciders**: Farid (owner)
**Related**: `docs/erp/tax-compliance.md`, `docs/erp/localization-standard.md`,
`docs/erp/gl-posting-contract.md` (the seam tax feeds).

## Context

ERPNext ships a region-neutral tax-template engine and zero Indonesia content. Per owner decision,
the Indonesia tax **rules are deferred** (authored later from DJP regulation with a tax reviewer).
The module must therefore ship the **engine + container shape** now — transcribable, region-neutral —
without baking Indonesian rates into it, and without baking tax into base invoice models.

## Decision

1. **Module named `backbone-tax`** (schema `tax`) — the `-id` in the plan meant "Indonesia"; the
   Indonesia-first framing lives inside as effective-dated seed data, not in the module name.
2. **Region-neutral engine, buildable now.** `TaxTemplate` + `TaxTemplateRow` (charge_type, rate,
   inclusive/exclusive, cumulative, **effective-dated**), `WithholdingCategory` (thresholded), and a
   `TaxEngine` that computes tax **lines**. Proven by a numeric oracle (exclusive/inclusive/cumulative
   VAT, effective-dating, withholding threshold).
3. **Tax is called; it never posts to the GL.** `calculate` / `resolve_withholding` return
   `TaxLine`s; the producing module (billing) attaches them to its `AccountingPost`. Tax is upstream
   of the GL seam, not a second seam. Account references are **logical FKs** to
   `accounting.Account.id`; tax imports nothing.
4. **Effective-dating is the versioning mechanism.** A rate change (11%→12%) is a new row with a new
   `effective_from`, never an edit to history — 11% and 12% coexist. The engine picks the row whose
   window contains the transaction date.
5. **Indonesia rules are DEFERRED overlay data.** PPN/PPh rates + categories, faktur-pajak numbering,
   e-Faktur/Coretax export, and SPT reporting are seed data + adapters + report shapes authored later.
   The overlay layers on without touching base models and is removable.

## Consequences

- One correct tax engine every producing module calls; base selling/billing/accounting models stay
  region-neutral.
- The Indonesia overlay ships as effective-dated seed rows + integrations, versioned and removable —
  a rate change is data, not code.
- Parking lot (per the spec): `TaxRule` auto-resolution (party/item/geo → template), the Indonesia
  rate/category seed, faktur-pajak/e-Faktur/bukti-potong/SPT overlay, customs/regional/excise taxes,
  multi-currency tax-base revaluation.
