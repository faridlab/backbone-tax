# backbone-tax — FSD

Schema (`schema/models/*.model.yaml`) is the SSoT.

## Entities

| Entity | Table | Key | Notes |
|--------|-------|-----|-------|
| TaxCategory | `tax.tax_categories` | `code` unique | `tax_kind` (vat/withholding/sales/other). |
| TaxTemplate | `tax.tax_templates` | `code` unique | `template_type` (sales/purchase), `is_inclusive`, `tax_category_id?`. |
| TaxTemplateRow | `tax.tax_template_rows` | — | `template_id`; `charge_type`; `rate`; `account_id?` (logical FK accounting); `is_withholding`; **`effective_from`/`effective_to`**; `sort_order`. |
| WithholdingCategory | `tax.withholding_categories` | — | `code`; `rate`; `threshold_amount`; `account_id?`; effective-dated. |

Tables live in the **`tax` Postgres schema**. Account references are **logical FKs** to
`accounting.Account.id` (`@exclude_from_foreign_key_check`).

## Endpoints

- **Guarded (recommended)** — `create_guarded_tax_routes(&TaxModule)`:
  - config: read + validated create (`POST /tax-categories`, `/tax-templates`, `/tax-template-rows`,
    `/withholding-categories`).
  - **compute**: `POST /tax/calculate` → `[TaxLine]`, `POST /tax/withholding` → `TaxLine?`.
- `TaxModule::all_crud_routes()` — generated full CRUD (admin only); `routes()` is `#[deprecated]`.

## The engine

`TaxEngine::calculate(template_id, base_amount, on_date) -> Vec<TaxLine>` and
`resolve_withholding(category_id, base_amount, on_date) -> Option<TaxLine>`. A `TaxLine` is
`{ account_id?, rate, tax_amount, is_withholding, description }`. Rules R1–R5 (`schema/hooks/tax.hook.yaml`):
charge computation, inclusive extraction, effective-rate selection, withholding threshold,
non-negative base. Money is exact IDR (2dp, round-half-up).

## The seam (tax contributes lines, not a posting)
A producing module (billing/selling/buying) calls the engine and attaches the returned lines to its
`AccountingPost` (the Financials-pillar contract owned by accounting). Tax never posts to the GL and
never imports a caller.

## Behavior specs
- Hooks: `schema/hooks/tax.hook.yaml` (engine + write rules; no lifecycle).
- Workflows: none (deferred faktur/e-Faktur overlay).
- Oracle: `docs/business-flows/` + `tests/features/tax.feature`; executable
  `tests/tax_golden_cases.rs` + `tests/integrity_probes.rs`.

## Non-goals
No GL posting, no Indonesia rate seed (deferred), no faktur/e-Faktur/SPT, no auto TaxRule resolution.
See [prd.md](prd.md) "Out".
