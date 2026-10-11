// This directory's extension (hand-written; ADR-0031). The generated mod.rs (lib.rs for
// src/) pulls it in with include!, so its lines resolve against this directory.

pub mod efaktur_service;
pub mod tax_engine;
pub mod tax_rounding;
pub mod tax_write_service;
pub use efaktur_service::{EFakturService, PostedForTax, TaxComplianceError};
pub use tax_engine::{
    DocumentTaxLine, DocumentTaxRequest, DocumentTaxRequestLine, DocumentTaxResult, DocumentType,
    TaxEngine, TaxError, TaxLine,
};
pub use tax_rounding::{distribute_delta_smoothly, round2, RoundingMethod};
pub use tax_write_service::{
    NewCategory, NewCompanySettings, NewRepartitionLine, NewRepartitionSplit, NewTag, NewTemplate,
    NewTemplateRow, NewWithholding, ReplaceRepartitionFamily, TaxWriteService,
};
