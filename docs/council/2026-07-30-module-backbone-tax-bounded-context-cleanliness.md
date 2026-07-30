<!-- front-matter
date: 2026-07-30
repo_type: module
unit: backbone-tax
focus: bounded-context-cleanliness
roster:
  standing: [chair, skeptic, steelman, yagni-business]
  context: [ddd-bounded-context, contract-seat]
  invited: [domain-expert]   # tax encodes real-world domain rules (e-Faktur / DJP / masa pajak)
subagents: [steelman, skeptic, chair]
scope: single unit (backbone-tax + its outward contracts)
-->

# Council — module:backbone-tax — focus: bounded-context-cleanliness

## Best call
**Verdict: NOT complete.** Wire `EFakturService` into the `TaxModule` public surface — add an `efaktur_service` field to the struct, construct it in `build()`, and re-export it from the `lib.rs` CUSTOM block — so the inbound audit mirror (write `TaxTransaction` when billing posts) becomes reachable through the module's own contract instead of a deep private path. This is the one in-module move that converts "correct-but-unreachable" into "owned capability."

This is NOT a both-sides call. The Steelman's "deferred-by-design" defense is legitimate for exactly one direction — the OUTBOUND SPT/faktur export surface, which `docs/fsd.md:45` explicitly lists as a non-goal. It does NOT cover the INBOUND mirror: `schema/models/tax_compliance.model.yaml:7-8` and `src/application/service/efaktur_service.rs:5-6` both declare "the composition layer calls this when billing emits SalesInvoicePosted/PurchaseInvoicePosted" — i.e. the enabled, promised path. That path is an orphan: `EFakturService` has no field in `TaxModule` (`src/lib.rs:65-73`), `build()` never constructs one (`src/lib.rs:141-191`), and it is absent from the crate-root CUSTOM re-export (`src/lib.rs:42-48`). The Skeptic falsified the Steelman's own load-bearing condition #4 workspace-wide — zero non-test callers of `record_tax_transaction` / `EFakturService` / `PostedForTax` exist anywhere under `/Users/faridlab/startapp/frameworks/metaphora`, and `SalesInvoicePosted` is subscribed by SELLING (it advances `billed_qty`) but never by tax. So a consumer of `TaxModule::builder().build()` cannot record a tax transaction without reaching into `application::service::efaktur_service::EFakturService` by hand — exactly what `tests/efaktur_seam.rs:34` does. That test green-lights SQL against a capability the public API cannot reach; it is a false green.

- Residual negative value: exposes reachability only, not end-to-end dispatch. Until a composition-layer subscriber for `SalesInvoicePosted`/`PurchaseInvoicePosted` is written (lives outside this crate; ~1–2 days of dispatcher + integration test, charged to the composition service not this module), `tax_transactions` / `efaktur_documents` / `tax_filing_periods` stay empty and the failure stays silent — now one layer closer but still vacuous. Added coupling is minimal: the field + its repo deps already live in-crate. Risk surface: a mis-wired subscriber could double-record, mitigated by the existing idempotency fence (`efaktur_service.rs:78-84`, `ON CONFLICT DO UPDATE`) and the DJP gapless allocator (`efaktur_service.rs:119-121`).
- Reversibility: easy. Remove the field, the construction, and the re-export.
- What would flip this: a CI assertion that a **non-test** caller of `EFakturService::record_tax_transaction` (or a `SalesInvoicePosted`/`PurchaseInvoicePosted` subscriber resolving to tax) exists in some composition/service crate. If green, the orphan is real-but-dispatched and the call drops to a no-op. The compile break (`src/lib.rs:200-201`) is a trivial uncommitted-edit prerequisite — HEAD `08db09e` is green and STILL carries this orphan, so fixing it does not change the verdict.

## Disagreement map
- **"Deferred-by-design" vs "unwritten orphan"** — Steelman says the 3 compliance entities are an immutable, event-driven record aggregate whose synchronous write surface is intentionally absent, so unmounted reads + no write route are expected; Skeptic says the inbound mirror is the *enabled* path and `EFakturService` isn't even in `TaxModule` wiring, so a consumer cannot fire it at all. Crux: **directionality** — OUTBOUND SPT/export is genuinely deferred (`docs/fsd.md:45`), INBOUND audit mirror is promised-enabled and unwired. The Steelman conflated the two; the Skeptic wins on the inbound claim.
- **Published `TaxQueryService` has no implementor** — Contract-seat says it's a broken published promise (read ops for all 7 entities, `src/exports/services.rs:18, 23-87`, with the only impl stub deleted this session); Yagni-business says don't build it until a consumer demands it. Crux: **is a trait in `exports/` a contract or a sketch** — the module's own rule ("other modules should ONLY depend on types defined here") makes it a contract, so a providerless promise is a defect, not a defer.
- **Wire dormant surfaces now vs YAGNI** — Skeptic reframes the 3 unmounted read routes (`create_{tax_transaction,tax_filing_period,e_faktur_document}_read_routes`) from "deferred export" to "symptom of no write path"; Yagni-business says mounting reads is premature until a real consumer exists. Crux: **mounting reads would expose empty tables** — so reads must follow the write path, not precede it.

## Recommendations (ranked by leverage)
| # | Move | Leverage | Residual negative | Reversibility | Evidence to flip |
|---|------|----------|-------------------|---------------|------------------|
| 1 | **Expose `EFakturService` through `TaxModule`** (struct field + `build()` construction + `lib.rs:42-48` CUSTOM re-export). Prereq: restore HEAD compile (`src/lib.rs:200-201`, ~5 min). | Converts the orphan from silently vacuous to reachable; makes the module's own contract honest. | Dispatcher still must be written in the composition crate; silent-empty-tables risk persists until then. | Easy | Non-test caller of `record_tax_transaction` in any composition crate. |
| 2 | **Add CI gate**: assert a non-test caller of `EFakturService::record_tax_transaction` (or a `SalesInvoicePosted`/`PurchaseInvoicePosted` subscriber resolving to tax) exists workspace-wide. | Kills the false-green forever; stops `efaktur_seam.rs` from certifying an unreachable capability. | Gate is red on landing until the subscriber is written — which is the point, but blocks "green" until composition layer ships. | Easy | Turn the gate green with a real subscriber. |
| 3 | **Resolve `TaxQueryService`** (`src/exports/services.rs:18, 23-87`): wire it to the existing 7 repos OR move it behind a `feature` until a consumer demands it. | Removes a providerless published contract. | If feature-gated: defers the promise rather than delivering it (acceptable under YAGNI). | Easy | A real cross-module consumer of the trait. |
| 4 | **Mount the 3 compliance read routes** — only AFTER move 1 + a live write path land. | Completes the read surface for the 3 record entities. | Mounting before the write path exists exposes empty tables and re-paints the symptom as done. | Easy | `tax_transactions` non-empty in any env. |
| 5 | **Rename the overloaded `period`** (rate-effective window vs `TaxFilingPeriod.period` / masa pajak). Naming proximity, not a semantic collapse — lowest priority. | Removes the one naming-smell the bounded-context review flagged. | Trivial rename churn across generated layers; must stay inside CUSTOM markers / schema YAML. | Costly (regen) | A real ambiguity-causes-bug instance. |

## Parking lot
- **`event_store` + `snapshot_store` full event-sourcing** for a low-frequency tax-config module — raised by yagni-business, scope: this module's infrastructure (the real over-build smell; out of lens).
- **e-Faktur gapless DJP allocator under concurrency** — load-test the row-lock + `UNIQUE(company,period,sequence) WHERE deleted_at IS NULL` backstop to confirm serialization. Raised by domain-expert, scope: this module.
- **Indonesia rate seed** — explicitly deferred at `docs/fsd.md:45`; revisit when in-scope.
- **OUTBOUND SPT/faktur export surface** — legitimately a non-goal per `docs/fsd.md:45`; mount read/export routes only when SPT becomes in-scope. Raised by steelman, scope: future feature.
