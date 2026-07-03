# Business Flow — Tax Calculation (the called engine)

> Owning module: `backbone-tax` · Engine in `src/application/service/tax_engine.rs`, config writes in
> `tax_write_service.rs`, served by `guarded_routes.rs`, proven by `tests/tax_golden_cases.rs`.
> Spec: `docs/erp/tax-compliance.md` (rules deferred).

Tax is **called, it does not own a document and never posts to the GL**. Selling/buying/billing ask
it to compute tax lines; the lines flow into an `AccountingPost` to `backbone-accounting`. Tax
contributes lines, not a posting — it is upstream of the GL seam.

## Actors
- **Tax admin** — configures categories, templates + effective-dated rate rows, withholding categories.
- **Producing modules** (billing/selling/buying) — call `calculate` / `resolve_withholding`.

## Flows

### Configure a tax template (admin)
- `POST /tax-categories`, `POST /tax-templates` (`{code, name, templateType, isInclusive}`), then
  `POST /tax-template-rows` (`{templateId, chargeType, rate, accountId?, effectiveFrom, effectiveTo?,
  sortOrder, isWithholding}`). Rules: R6 template exists; R7 valid window; R8 unique code.
- `POST /withholding-categories` (`{code, name, rate, thresholdAmount, accountId?, effectiveFrom}`).

### Calculate tax on a base (the seam)
- `POST /tax/calculate` `{ templateId, baseAmount, onDate }` → array of tax lines
  `{ accountId, rate, taxAmount, isWithholding, description }`.
- Engine (R1–R3): pick each row effective on `onDate`; `on_net_total` = rate% of net;
  `on_previous_row_total` = rate% of (net + prior charges); `actual` = fixed. If the template is
  **inclusive**, the base is the gross and the tax is extracted (net = gross / (1 + Σrates)).
- The caller (billing) attaches these lines to its `AccountingPost` (e.g. Cr PPN Output Payable).

### Resolve withholding (purchase, PPh)
- `POST /tax/withholding` `{ categoryId, baseAmount, onDate }` → a negative line if
  `base ≥ threshold` (R4), else `null`. The caller attaches it as a deduction (Cr PPh Payable).

## Golden numbers (`tests/tax_golden_cases.rs`)
- PPN 11% exclusive on 1,000,000 → **110,000**.
- PPN 11% inclusive gross 1,110,000 → extracted **110,000** (net 1,000,000).
- Effective-dating: 11% before 2025-01-01, 12% on/after → 110,000 vs **120,000**.
- Cumulative 10% surcharge on (net + PPN) → **111,000**.
- PPh 23 2% above 1,000,000 threshold → **-100,000**; below → no line.

## Deferred (Indonesia overlay — authored later from DJP regulation)
Seed PPN 11%→12% rows, PPh 21/22/23/26/4(2) categories, faktur-pajak numbering, e-Faktur/Coretax
export, bukti potong, SPT reporting. See [golden-cases.md](golden-cases.md).
