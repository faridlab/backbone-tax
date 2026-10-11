// This directory's extension (hand-written; ADR-0031). The generated mod.rs (lib.rs for
// src/) pulls it in with include!, so its lines resolve against this directory.

// The hand-owned request-pool shim (the composing service's tenant pool
// resolution): a generated-tree declaration the regenerator drops, so it
// lives in the preserved block (#447 cause-2 class).
pub mod request_pool;
pub use application::service::{
    DocumentTaxLine, DocumentTaxRequest, DocumentTaxRequestLine, DocumentTaxResult, DocumentType,
    NewCategory, NewCompanySettings, NewRepartitionLine, NewRepartitionSplit, NewTag, NewTemplate,
    NewTemplateRow, NewWithholding, ReplaceRepartitionFamily, TaxEngine, TaxError, TaxLine,
    TaxWriteService,
};
// Document-grade rounding primitives (round_globally redistribution math) —
// public so the rounding unit oracle can pin them from integration tests.
pub use application::service::{distribute_delta_smoothly, round2, RoundingMethod};
// The e-Faktur + tax-recording seam. The composition ACL calls
// `EFakturService::record_tax_transaction` when billing emits a posted event —
// this is the inbound audit-mirror write path (see docs/fsd.md).
pub use application::service::{EFakturService, PostedForTax, TaxComplianceError};
pub use presentation::http::create_guarded_tax_routes;
