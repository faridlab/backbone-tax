# Tax Workflows

Tax has **no workflows** — it is a called engine, not a document owner. It computes tax lines
(`calculate`, `resolve_withholding`) that a caller attaches to an AccountingPost. The Indonesia
overlay (faktur-pajak numbering, bukti potong, SPT) is a **deferred** workflow surface authored
later from DJP regulation (docs/erp/tax-compliance.md).
