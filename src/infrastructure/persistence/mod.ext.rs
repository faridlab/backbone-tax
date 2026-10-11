// This directory's extension (hand-written; ADR-0031). The generated mod.rs (lib.rs for
// src/) pulls it in with include!, so its lines resolve against this directory.

// The hand-written tax SQL's parameter/projection types. Every repository listed here is declared
// `user_owned` in metaphor.codegen.yaml — see tax_write_service.rs / efaktur_service.rs, which
// orchestrate them.
pub use company_tax_settings_repository::CompanyTaxSettingsRecord;
pub use e_faktur_document_repository::{
    EFakturDocumentRow, EFakturExportRow, NewEFakturDocumentRow,
};
pub use tax_category_repository::NewTaxCategoryRow;
pub use tax_filing_period_repository::{AllocatedSequence, FilingPeriodRow};
pub use tax_repartition_line_repository::{NewTaxRepartitionLineRecord, RepartitionLineRecord};
pub use tax_tag_repository::NewTaxTagRow;
pub use tax_template_repository::NewTaxTemplateRow;
pub use tax_template_row_repository::NewTaxTemplateRowRecord;
pub use tax_transaction_repository::NewTaxTransactionRow;
pub use withholding_category_repository::NewWithholdingCategoryRow;
