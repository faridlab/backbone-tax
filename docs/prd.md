# backbone-tax — PRD

> The tax **engine** (region-neutral) + Indonesia **overlay** shape. It computes tax lines that
> flow into an `AccountingPost`; it never posts to the GL. Indonesia rules are **deferred** — this
> module ships the container/engine now, the DJP regulation is authored later. This pillar is the
> product's key differentiator (docs/erp/tax-compliance.md).

## Problem

ERPNext ships a generic tax-template engine and **zero Indonesia content**; its only Indonesia
artifact (`id_chart_of_accounts.json`) is community-contributed and unreliable. Baking a PPN field
into invoices couples tax into every base model. We need one module that (a) computes tax lines from
a region-neutral engine, and (b) layers Indonesian rates/rules as removable, effective-dated overlay
data — so base selling/billing/accounting models stay region-neutral.

## Scope

**In (the engine + container — buildable now):**
- `TaxCategory` — classification a rule/template resolves to (VAT / withholding / …).
- `TaxTemplate` + `TaxTemplateRow` — a named set of charge rows with **effective-dated** rates,
  inclusive/exclusive, cumulative (`on_net_total` / `on_previous_row_total` / `actual`).
- `WithholdingCategory` — thresholded deduction-at-source with an effective-dated rate.
- The **engine**: `calculate(template, base, date) → tax lines` and
  `resolve_withholding(category, base, date) → line?`. Exact IDR money (2dp, round-half-up).
- Validated config writes + read/compute HTTP surface.

**Out (deferred — authored later from DJP regulation):**
- Indonesian **rates/categories** (PPN 11%→12%, PPh 21/22/23/26/4(2)) — seed data, effective-dated.
- **faktur pajak** numbering, **e-Faktur/Coretax** export, **bukti potong**, **SPT** reporting.
- `TaxRule` auto-resolution (party/item/geo → template) — the caller selects a template for now.
- Customs/import (PIB), regional taxes (PBB/BPHTB), excise (cukai).

## Personas
- **Tax admin** — configures templates + effective-dated rates.
- **Producing modules** (billing/selling/buying) — call the engine; attach lines to their AccountingPost.

## Success criteria
- Correct exclusive/inclusive/cumulative computation + effective-dated rate selection, pinned by a
  numeric oracle (golden cases).
- Tax never posts to the GL; it returns lines. Rates are effective-dated (11% and 12% coexist).
- Dedicated `tax` Postgres schema; account references are logical FKs to accounting; no Cargo edges.
- The Indonesia overlay layers on without touching base models and is removable.

## Indonesia-first notes
The engine is region-neutral; Indonesia is **seed data + integration + report shapes**, versioned and
effective-dated, authored later with a tax-domain reviewer. A rate change (11%→12%) is an
effective-dated row edit, never a code change.
