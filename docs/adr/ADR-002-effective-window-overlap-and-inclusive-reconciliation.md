# ADR-002: No overlapping effective windows + exact inclusive reconciliation

**Status**: Accepted — **Applied 2026-07-03**
**Deciders**: Farid (owner), council (module:backbone-tax, focus=maturity, 2026-07-03)
**Related**: ADR-001 (tax engine boundary)

## Context

A maturity council confirmed the engine's boundary, language, regen-safety, and the routes() seal
are strong, but found a **HIGH silent financial defect** in the engine itself (the exact thing in
scope now). `calculate()` selected **all** rows whose window contained the date and summed **every**
one — no `DISTINCT ON`, no `LIMIT`. Nothing (write path, DB, or read path) enforced the invariant
"exactly one applicable row per `sort_order` per date". The most common effective-dating mistake —
adding the new 12% row on 2025-01-01 **without closing** the old 11% row's open (NULL) window — left
two rows both effective, so `calculate(1,000,000)` returned **two lines: 110,000 AND 120,000 =
230,000 tax (a 23% effective rate), no error**, flowing into the GL. The inclusive path was worse
(`pct_sum` summed 11+12=23 → `net = gross/1.23`, matching neither regime). Reproduced live.

Secondary: inclusive tax lines were computed as `round(net × rate)`, not `gross − net`, so
`Σ lines` could differ from `gross − net` by a cent — making the caller's `AccountingPost`
unbalanced, which accounting **rejects** (a cross-seam failure).

## Decision

1. **Overlapping effective windows are unrepresentable.** An `EXCLUDE USING gist` constraint (a
   protected hand migration, `btree_gist`) forbids two rows at the same `(template_id, sort_order)`
   — and two `withholding_categories` of the same `code` — with overlapping
   `daterange(effective_from, coalesce(effective_to,'infinity'), '[]')`. `add_row` /
   `create_withholding` also reject overlaps at write time with a typed `overlapping_effective_window`
   (422). Belt-and-suspenders: the sanctioned path rejects, and the DB makes it impossible.
2. **`calculate` is deterministic on the read path too.** `SELECT DISTINCT ON (sort_order) …
   ORDER BY sort_order, effective_from DESC` — one row per `sort_order`, newest-effective wins — so
   even a row that somehow bypassed the constraint can never double-charge (and the inclusive
   `pct_sum` can never double-count).
3. **Inclusive reconciles exactly.** For inclusive templates `net = money(gross / (1 + Σon_net%))`
   and the tax lines sum to **exactly** `gross − net` — the last `on_net` line absorbs the rounding
   residual, so `Σ lines == gross` for the caller's balanced posting. Proven across odd grosses.
4. **Inclusive templates support only `on_net_total` rows.** A cumulative/`actual` row in an
   inclusive template has no defined grossing-up basis → rejected with
   `inclusive_cumulative_unsupported`, rather than silently computing a wrong net.

## Consequences

- The engine can no longer double-charge from a plausible admin edit, nor emit an unbalanced
  inclusive posting. Three new golden cases (TGC-7/8/9) lock it; the suite no longer certifies only
  the disjoint-window happy path.
- The `EXCLUDE` constraint requires `btree_gist` on the tenant Postgres (one migration line) — a
  deploy prerequisite; a one-time audit of any pre-existing rows for overlap is advisable.
- Residual / parking lot (per the council): `charge_type='actual'` cumulative-base semantics
  (document/pin), `account_id` logical-FK validation (consumer/ACL), the `TaxRule`
  party/item/geo resolver + the Indonesia rate/category seed (deliberately deferred).
