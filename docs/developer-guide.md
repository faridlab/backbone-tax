# Developer guide — backbone-tax

> **Reader:** App developer (a producing module — billing/selling/buying — or an admin tool).
> **Mode:** tutorial → how-to. Get from "installed" to "I computed a tax line" fast, then look up
> the recipe you need. Every command and snippet here is drawn from the module's real surface
> (`Cargo.toml`, `guarded_routes.rs`, `config/application.yml`); where a step wasn't executed in a
> live DB, it says so.

Tax computes **lines**; it never posts to the general ledger. You call `calculate` / `withholding`,
get back `[TaxLine]`, and attach those lines to your own `AccountingPost`. Keep that in mind and the
rest follows.

## Install

`backbone-tax` is a library crate consumed by a `backend-service`. Add it as a path or git
dependency in the service's `Cargo.toml`:

```toml
[dependencies]
backbone-tax = { path = "../backbone-tax" }   # or git = "…", tag = "v0.1.2"
```

The module needs a PostgreSQL pool and its migrations applied. It owns the **`tax`** Postgres
schema; migrations emit `CREATE SCHEMA tax` and qualify every table as `tax.<table>`.

```bash
# from inside backbone-tax/
metaphor migration run         # apply migrations (creates the `tax` schema + tables)
metaphor dev test              # sanity-check the module builds and the oracle passes
```

> **Prerequisite (one line, one time):** ADR-002's overlap guard uses an `EXCLUDE USING gist`
> constraint that requires the `btree_gist` extension on the target Postgres. Migration
> `20260426220020_tax_effective_overlap_exclude.up.sql` enables it; ensure your DB role may
> `CREATE EXTENSION`, or have a DBA pre-create it.

## Quickstart — compute a tax line in 3 calls

The smallest thing that runs: create a template, add an 11% row, compute. Uses the **guarded**
routes (the recommended mount). Substitute your host; these are the real endpoints from
[`guarded_routes.rs`](../src/presentation/http/guarded_routes.rs).

**1. Create a sales template (exclusive):**
```bash
curl -sX POST localhost:8080/tax-templates \
  -H 'content-type: application/json' \
  -d '{"code":"PPN_OUT","name":"PPN Keluaran","templateType":"sales","isInclusive":false}'
# → 201 {"id":"<TEMPLATE_ID>"}
```

**2. Add an effective-dated 11% row:**
```bash
curl -sX POST localhost:8080/tax-template-rows \
  -H 'content-type: application/json' \
  -d '{"templateId":"<TEMPLATE_ID>","chargeType":"on_net_total","rate":11.0,
       "effectiveFrom":"2022-04-01","sortOrder":0,"description":"PPN Keluaran 11%"}'
# → 201 {"id":"<ROW_ID>"}
```

**3. Compute tax on a 1,000,000 base as of a date:**
```bash
curl -sX POST localhost:8080/tax/calculate \
  -H 'content-type: application/json' \
  -d '{"templateId":"<TEMPLATE_ID>","baseAmount":1000000,"onDate":"2023-06-01"}'
# → 200 [{"accountId":null,"rate":"11.0000","taxAmount":"110000.00",
#          "isWithholding":false,"description":"PPN Keluaran 11%"}]
```

You now have a `TaxLine`. **Attach it to your `AccountingPost`** — map `taxAmount` to a GL line
against `accountId` — and post *that*. Tax's job ends at the line.

> The numbers above are the module's documented behavior (exclusive `on_net_total`: 11% of
> 1,000,000 = 110,000.00, exact IDR). They match golden case TGC-1 in
> [business-flows/golden-cases.md](business-flows/golden-cases.md); the `curl` transcript itself
> was not run against a live server here.

## Wire the module into a service (in-process)

If you're composing rather than calling over HTTP, build the module and merge its guarded router.
From [`lib.rs`](../src/lib.rs) and [`extension-guide.md`](extension-guide.md):

```rust
use backbone_tax::{TaxModule, create_guarded_tax_routes};

let tax = TaxModule::builder()
    .with_database(pool.clone())
    .build()?;

// Recommended: read config + validated writes + compute.
let app = axum::Router::new().merge(create_guarded_tax_routes(&tax));

// In-process compute (billing's invoice-post composer calls this directly):
let lines = tax.tax_engine
    .calculate(template_id, net_total, invoice_date)
    .await?;
```

## Recipes

### "Change a rate from 11% to 12%" — never edit history
Insert a **new row**, don't mutate the old one. Close the old window, open the new one so they don't
overlap (an overlap is rejected — see Troubleshooting):
```bash
# close the 11% row: PATCH its effectiveTo to 2024-12-31 (via the row's update endpoint)
# then add the 12% row:
curl -sX POST localhost:8080/tax-template-rows -H 'content-type: application/json' \
  -d '{"templateId":"<TEMPLATE_ID>","chargeType":"on_net_total","rate":12.0,
       "effectiveFrom":"2025-01-01","sortOrder":0,"description":"PPN Keluaran 12%"}'
```
`calculate(..., onDate:"2024-06-01")` returns 11%; `onDate:"2025-06-01")` returns 12%. Both coexist.

### "Compute an inclusive (tax-in) price"
Set `isInclusive:true` on the template. The engine treats `baseAmount` as the gross and *extracts*
the tax: `net = gross / (1 + Σ on_net%)`, and the lines sum to exactly `gross − net`. **Inclusive
templates accept only `on_net_total` rows** — a cumulative or `actual` row is rejected with
`inclusive_cumulative_unsupported`.

### "Resolve a withholding deduction (PPh-style)"
Withholding is a separate category with a threshold. Under threshold → no line (`null`):
```bash
curl -sX POST localhost:8080/withholding-categories -H 'content-type: application/json' \
  -d '{"code":"PPH23_SVC","name":"PPh 23 services","rate":2.0,"threshold_amount":1000000,
       "effective_from":"2022-01-01"}'
curl -sX POST localhost:8080/tax/withholding -H 'content-type: application/json' \
  -d '{"categoryId":"<CAT_ID>","baseAmount":5000000,"onDate":"2023-06-01"}'
# → 200 {"accountId":null,"rate":"2.0000","taxAmount":"-100000.00","isWithholding":true, …}
```
The `taxAmount` is **negative** — withholding is a deduction at source.

### "Cumulative charges (tax on tax)"
Use `chargeType:"on_previous_row_total"` with `sortOrder` to order the rows. Each such row's rate
applies to `net + all prior charge amounts`. Use `chargeType:"actual"` when `rate` is a fixed amount
(a flat fee), not a percentage.

### "Just read the config"
The guarded router mounts read routes for all four entities (list/get). For the full generated CRUD
(create/update/patch/soft-delete/restore/bulk/upsert — 12 endpoints/entity) use
`TaxModule::all_crud_routes()`, but **only** in trusted/admin/seed contexts: it has no domain
validation and can create invalid rows.

## Configuration

`config/application.yml` (overridden by `application-dev.yml` / `application-prod.yml`):

| Key | Meaning |
|-----|---------|
| `database.url` | Postgres DSN; the module uses the `tax` schema within it |
| `server.port` / `grpc_port` | HTTP `8080`, gRPC `50051` (gRPC codegen is disabled for this module) |
| `features.soft_delete` | on — deletes set `deleted_at`; the engine filters `deleted_at IS NULL` |
| `entities.*.pagination` | list default/max limits per entity |

Validate config with the CLI's `config` command (`metaphor config --help`).

## Troubleshooting

| Symptom | HTTP | Cause | Fix |
|---------|------|-------|-----|
| `template_not_found` | 422 | template id wrong or soft-deleted | check the id; deleted templates are excluded |
| `no_effective_rate` | 422 | no row's window contains `onDate` | add/adjust a row so `effective_from ≤ onDate ≤ effective_to` |
| `overlapping_effective_window` | 422 | two rows (same `sort_order`) or two withholding categories (same `code`) overlap in time | close the old row's `effective_to` before opening the new one |
| `inclusive_cumulative_unsupported` | 422 | inclusive template has a non-`on_net_total` row | inclusive templates support only `on_net_total`; make it exclusive or drop the row |
| `negative_base` | 422 | `baseAmount < 0` | the base must be ≥ 0 |
| `duplicate_code` | 422 | a category/template/withholding `code` already exists | codes are unique (per non-deleted) |
| `internal_error` | 500 | DB error (e.g. `btree_gist` missing) | check migrations applied and the extension exists |
| Caller's post is rejected as unbalanced | — | you summed lines yourself and lost a cent | trust the returned lines; for inclusive, `Σ lines == gross` by construction |

Error codes are defined in [`tax_engine.rs`](../src/application/service/tax_engine.rs) (`TaxError::code`)
and cross-referenced in [schema/ERROR_CODES.md](schema/ERROR_CODES.md).

## Next
- The integration contract and the Indonesia overlay: [extension-guide.md](extension-guide.md).
- Why tax returns lines instead of posting: [ADR-001](adr/ADR-001-tax-engine-boundary.md).
- Vocabulary: [glossary.md](glossary.md).
