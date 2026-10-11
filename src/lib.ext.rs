//! The module's extension (hand-written; ADR-0031): what the module adds beside its
//! generated services. The generated module and builder carry `ModuleExt` and
//! `ModuleBuilderExt` and dereference to them, so their fields read as `module.field`.

#[allow(unused_imports)]
use super::*;

/// State the module adds; read through the generated module's `Deref`.
pub struct ModuleExt {
    /// The region-neutral tax engine (compute tax lines). Public so producing
    /// modules (billing/selling/buying) can call it in-process and attach the
    /// returned lines to their own AccountingPost (FSD: "tax contributes lines,
    /// not a posting").
    pub tax_engine: Arc<TaxEngine>,
    /// Validated tax-config writes (mounted as guarded HTTP routes).
    pub tax_write_service: Arc<TaxWriteService>,
    /// The e-Faktur + tax-recording seam. Public so the composition ACL can call
    /// `record_tax_transaction` when billing emits SalesInvoicePosted /
    /// PurchaseInvoicePosted — the inbound audit-mirror write path.
    pub efaktur_service: Arc<EFakturService>,
}

/// State the builder adds.
pub struct ModuleBuilderExt {
}

impl Default for ModuleBuilderExt {
    fn default() -> Self {
        Self {
        }
    }
}

impl ModuleBuilderExt {
    /// Build the extension's state from what the generated build made.
    #[allow(unused_variables, clippy::redundant_clone)]
    pub(crate) fn build(self, parts: &ModuleParts<'_>) -> anyhow::Result<ModuleExt> {
        let db_pool = parts.db_pool.clone();
        let tax_engine = Arc::new(TaxEngine::new(db_pool.clone()));
        let tax_write_service = Arc::new(TaxWriteService::new(db_pool.clone()));
        let efaktur_service = Arc::new(EFakturService::new(db_pool.clone()));
        Ok(ModuleExt {
            tax_engine,
            tax_write_service,
            efaktur_service,
        })
    }
}
