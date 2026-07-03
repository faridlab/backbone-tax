# Tax — Golden Cases (the numeric oracle)

Exact expected results, mirroring `tests/tax_golden_cases.rs` + `tests/integrity_probes.rs`. Money
is exact IDR (2 decimals, round-half-up). Rates are *sample* values, NOT seeded Indonesia regulation.

## Engine (`tests/tax_golden_cases.rs`)

| Case | Input | Expected |
|------|-------|----------|
| **TGC-1** | exclusive PPN 11% on 1,000,000 | one line `110,000.00`. |
| **TGC-2** | inclusive gross 1,110,000 @ 11% | extracted tax `110,000.00` (net 1,000,000). |
| **TGC-3** | rows 11% (…2024-12-31) + 12% (2025-01-01…); calc on 2024-06 vs 2025-06 | `110,000` vs `120,000`. |
| **TGC-4** | PPN 11% + cumulative 10% on (net+PPN) | `110,000` then `111,000`. |
| **TGC-5** | PPh 23 2%, threshold 1,000,000; base 5,000,000 vs 500,000 | `-100,000` vs none. |
| **TGC-6** | unknown template / no effective row / negative base | `template_not_found` / `no_effective_rate` / `negative_base`. |
| **TGC-7** | add a 12% row without closing the old 11% open window (overlap) | `422 overlapping_effective_window`; `calculate` returns exactly **one** line (110,000, no double-charge). |
| **TGC-8** | inclusive 11% on odd grosses (1,111,111 / 999,999 / …) | `Σ lines == gross - net` exactly (net + tax = gross). |
| **TGC-9** | inclusive template with a cumulative row | `422 inclusive_cumulative_unsupported`. |

## Route surface (`tests/integrity_probes.rs`)

| Case | Input via guarded routes | Expected |
|------|--------------------------|----------|
| **IGC-1** | `POST /tax-templates/bulk` (generic) | `405/404` — config only via validated create. |
| **IGC-2** | `POST /tax-template-rows` with a missing template | `422 template_not_found`. |
| **IGC-3** | row with `effectiveTo < effectiveFrom` | `422 invalid_date_range`. |
| **IGC-4** | `POST /tax/calculate` (template + row seeded) | `200`; a `110,000` line. |

## Conventions
- The engine returns tax **lines**; it never posts to the GL — the caller attaches lines to an
  `AccountingPost`.
- Rates are effective-dated (`effective_from`/`effective_to`); a change (11%→12%) is a new row, never
  an edit to history.
- Withholding lines and `is_withholding` rows are negative (deductions).
