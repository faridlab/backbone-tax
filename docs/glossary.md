# Glossary — backbone-tax (ubiquitous language)

> **Reader:** all. **Mode:** reference. One term, one definition, used consistently across the whole
> handbook and the code. If a term here disagrees with a page, this glossary wins — fix the page.
> Terms are drawn from `schema/models/*.model.yaml`, `tax_engine.rs`, and the two ADRs.

### TaxLine
The engine's **output** — a single computed charge: `{ account_id?, rate, tax_amount, is_withholding,
description }`. `calculate` returns `Vec<TaxLine>`; `resolve_withholding` returns `Option<TaxLine>`.
A line is *not* a posting — the caller maps it onto an [AccountingPost](#accountingpost). Withholding
lines carry a **negative** `tax_amount`.

### The engine (`TaxEngine`)
The hand-authored, region-neutral compute core (`src/application/service/tax_engine.rs`). Two
operations: `calculate(template_id, base_amount, on_date)` and `resolve_withholding(category_id,
base_amount, on_date)`. It reads config from Postgres and returns lines. It holds no Indonesia rates
— those are [seed data](#the-indonesia-overlay).

### The seam
The integration contract between tax and a producing module: `calculate(context) → [TaxLine]`. Tax
returns lines; the caller (billing/selling/buying) attaches them to its own AccountingPost and posts
*that*. Tax imports no caller; the caller imports no tax internals. This is the boundary
[ADR-001](adr/ADR-001-tax-engine-boundary.md) protects.

### AccountingPost
The Financials-pillar contract **owned by accounting**, not by tax. A producing module builds one
per business event (e.g. an invoice) and posts it to the general ledger. Tax contributes lines to
it; tax never constructs or posts one.

### GL (general ledger)
The double-entry ledger in `backbone-accounting`. **Tax never posts to the GL.** Tax is *upstream* of
the GL seam — it produces lines that the caller folds into an AccountingPost, which the caller posts.

### TaxCategory
Config master data: a classification a template or rule resolves to (`code`, `name`, `tax_kind`,
`status`). `tax_kind ∈ {vat, withholding, sales, other}`. Region-neutral shape; Indonesia values
(PPN_STD, PPH23, …) are seeded later.

### TaxTemplate
A named, `code`-unique set of charge rows (`template_type ∈ {sales, purchase}`,
`is_inclusive`, optional `tax_category_id`). The engine applies a template's effective rows to a base
to produce lines.

### TaxTemplateRow
One charge row of a template: `charge_type`, `rate`, optional `account_id`, `is_withholding`,
`effective_from`/`effective_to`, `sort_order`. The unit that carries a rate and its
[effective window](#effective-dating).

### WithholdingCategory
A thresholded deduction-at-source category: `code`, `rate`, `threshold_amount`, optional
`account_id`, effective-dated. Below `threshold_amount` the engine returns no line. Indonesia's PPh
21/22/23/26/4(2) are modeled as these (seeded later).

### charge_type
What base a row's `rate` applies to:
- **`on_net_total`** — rate% of the net (pre-tax) base. The only type an inclusive template allows.
- **`on_previous_row_total`** — rate% of (net + all prior charge amounts): **cumulative** (tax on tax).
- **`actual`** — the `rate` field holds a **fixed amount**, not a percentage.

### Inclusive vs exclusive
- **Exclusive** (`is_inclusive = false`) — the base is pre-tax; tax is added on top.
- **Inclusive** (`is_inclusive = true`) — the base already contains the tax; the engine **extracts**
  it: `net = money(gross / (1 + Σ on_net%))`. The lines sum to **exactly** `gross − net` (the last
  `on_net` line absorbs the rounding residual) so the caller's post balances. Inclusive templates
  accept only `on_net_total` rows — see [ADR-002](adr/ADR-002-effective-window-overlap-and-inclusive-reconciliation.md).

### Effective-dating
The module's **versioning mechanism**. A row/category is valid over `[effective_from, effective_to]`
(`effective_to = null` means open-ended). A rate change (11% → 12%) is a **new row with a new
`effective_from`**, never an edit to history — both coexist, and the engine picks the row whose
window contains the transaction date. This is why there is no status/lifecycle state machine.

### Overlapping effective window
Two rows at the same `(template_id, sort_order)` — or two withholding categories of the same `code`
— whose date windows intersect. **Unrepresentable**: rejected at write time (`422
overlapping_effective_window`) *and* forbidden by an `EXCLUDE USING gist` DB constraint. On the read
path, `DISTINCT ON (sort_order) … ORDER BY effective_from DESC` picks exactly one row regardless.
Prevents silent double-charging ([ADR-002](adr/ADR-002-effective-window-overlap-and-inclusive-reconciliation.md)).

### Logical FK
A UUID reference to another module's row **without** a database foreign key or a Cargo dependency.
`account_id` on rows/withholding is a logical FK to `accounting.Account.id`, marked
`@exclude_from_foreign_key_check`. It keeps tax importing nothing from accounting.

### Money / exact IDR
All amounts are rounded to **2 decimals, round-half-up** (`RoundingStrategy::MidpointAwayFromZero`),
applied per line. `rust_decimal`, never floats.

### The Indonesia overlay
The **deferred** Indonesian content: PPN 11%→12% template rows, PPh withholding categories,
faktur-pajak numbering, e-Faktur/Coretax export, SPT reporting. It is **seed data + adapters +
report shapes**, authored later from DJP regulation — *not* module code. It layers on without
touching base models and is removable. A rate change is a data edit, never a code change.

### user-owned / regen-safe
A file the code generator never touches. Declared in `metaphor.codegen.yaml` (`tax_engine.rs`,
`tax_write_service.rs`, `guarded_routes.rs`, golden tests, the overlap migration, `docs/**`) or
enclosed in `// <<< CUSTOM … // END CUSTOM` markers. Everything else in `src/` is regenerated from
the schema on `metaphor schema schema generate --force`.

### Guarded routes
`create_guarded_tax_routes(&module)` — the **recommended** mount: read config + validated writes
(`TaxWriteService`) + compute (`TaxEngine`). Contrast the **unguarded** `all_crud_routes()` (full
generated CRUD, no validation, admin/seed-only) and the `#[deprecated]` `routes()` alias.

### Golden case (TGC-*) / oracle
An exact expected numeric result that pins engine behavior, listed in
[business-flows/golden-cases.md](business-flows/golden-cases.md) and encoded in
`tests/tax_golden_cases.rs`. The oracle, not the implementation, is the source of truth for what a
computation *should* return; any behavior change moves a golden case first.
