# ADR-003: Tax compliance — TaxTransaction audit mirror + gapless e-Faktur numbering

**Status**: Accepted — Applied 2026-07-26
**Related**: billing ADR-001 (boundary), ADR-002 (the seam pattern)

## Context

Billing carries tax as a removable `InvoiceTaxLine` overlay but never *computes* it and never
*numbers* it. For Indonesia compliance (PPN/PPh + e-Faktur), tax needs its own audit record per
posted invoice and a gapless DJP-format numbering sequence. This ADR records the boundary.

## Decision

1. **Tax owns the audit mirror + the numbering.** `TaxTransaction` records each posted invoice's tax
   result (output/input/withholding totals) — immutable, linked to billing's invoice by a logical FK.
   `EFakturDocument` carries the DJP-format number (`010.NNN-NN.YYYYYYYY`). `TaxFilingPeriod` is the
   monthly masa pajak allocator (gapless sequence via row-locked `UPDATE ... RETURNING`).

2. **Gapless sequence.** `TaxFilingPeriod.next_sequence` is atomically incremented
   (`UPDATE ... SET next_sequence = next_sequence + 1 RETURNING next_sequence - 1`) inside the
   EFakturDocument insert transaction. Concurrent posts serialize on the per-period row. DJP requires
   no gaps; voiding keeps the row (status=voided, sequence preserved).

3. **Idempotent recording.** `TaxTransaction` unique on `(company, invoice_ref, invoice_kind)` — a
   re-delivery of the same posted event reuses the existing transaction. The e-Faktur assignment
   checks for an existing `efaktur_document_id` before allocating (no double-numbering).

4. **Zero cargo edges.** Tax never imports billing. The composition ACL passes the posted invoice
   data to `EFakturService::record_tax_transaction`. Tax owns its own `InvoiceKind` enum (string-tagged
   on the seam, not imported from billing).

## Consequences

- Proven by `tests/efaktur_seam.rs` (TSEAM-1): sales → TaxTransaction recorded + e-Faktur
  `010.000-07.00000001` assigned; second sales → sequence 2 (gapless); re-delivery → same
  transaction + same e-Faktur (idempotent); purchase → no e-Faktur.
- Deferred: DJP/Coretax filing submission, cross-border VAT (OSS/NOSS), PPh 21 (payroll-side),
  purchase faktur matching, bukti potong generation, SPT report assembly.
