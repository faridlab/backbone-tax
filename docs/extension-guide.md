# backbone-tax — Extension Guide

How a producing module (billing/selling/buying) uses the tax engine, and how the Indonesia overlay
layers on later.

## Composing into a service

```rust
use backbone_tax::{TaxModule, create_guarded_tax_routes};

let tax = TaxModule::builder().with_database(pool.clone()).build()?;
let app = axum::Router::new().merge(create_guarded_tax_routes(&tax));
```

`tax.tax_engine` is exposed to call `calculate` / `resolve_withholding` in-process (a composer wires
billing's invoice-post to it). `all_crud_routes()` (generic CRUD) is admin-only; `routes()` is
`#[deprecated]`.

## The seam — attach lines to an AccountingPost
```
Billing: invoice net total ─▶ tax.calculate(template, net, date) ─▶ [TaxLine]
        │  map each TaxLine → an AccountingPost line (account_id, tax_amount as debit/credit)
        ▼
Accounting: GL POST (Dr A/R · Cr Revenue · Cr PPN Output Payable)
```
Tax returns **lines**, never a posting. The caller owns the AccountingPost. Tax never imports the
caller; the caller never imports tax's internals — the contract is `calculate(context) → [TaxLine]`.

## Public / stable surface
- **Entities & DTOs** — TaxCategory/TaxTemplate/TaxTemplateRow/WithholdingCategory + generated DTOs.
- **Engine** — `TaxEngine::{calculate, resolve_withholding}`, `TaxLine`, `TaxError`.
- **Validated config writes** — `TaxWriteService`, `NewCategory`/`NewTemplate`/`NewTemplateRow`/
  `NewWithholding`, and `create_guarded_tax_routes`.
- **Logical FK** — `account_id` on rows/withholding are logical FKs to `accounting.Account.id`.

## Layering the Indonesia overlay (later)
The overlay is **seed data + effective-dated rows**, not code: seed PPN 11%/12% template rows, PPh
21/22/23/26/4(2) withholding categories, and (deferred) faktur-pajak numbering + e-Faktur/Coretax
adapters + SPT read models. A rate change is an effective-dated row, never a code edit — the overlay
is removable without touching base models.

## Regeneration safety
`tax_engine.rs`, `tax_write_service.rs`, `guarded_routes.rs`, tests, and `docs/**` are `user_owned`
in `metaphor.codegen.yaml` and survive `metaphor schema schema generate --force`. Own schema: `tax`.
