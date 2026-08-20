use chrono::{DateTime, NaiveDate, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

use super::AuditMetadata;
use super::InvoiceKind;
use super::TaxTransactionSource;
use super::TaxTransactionStatus;

/// Strongly-typed ID for TaxTransaction
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct TaxTransactionId(pub Uuid);

impl TaxTransactionId {
    pub fn new(id: Uuid) -> Self {
        Self(id)
    }
    pub fn generate() -> Self {
        Self(Uuid::new_v4())
    }
    pub fn into_inner(self) -> Uuid {
        self.0
    }
}

impl std::fmt::Display for TaxTransactionId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::str::FromStr for TaxTransactionId {
    type Err = uuid::Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self(Uuid::parse_str(s)?))
    }
}

impl From<Uuid> for TaxTransactionId {
    fn from(id: Uuid) -> Self {
        Self(id)
    }
}

impl From<TaxTransactionId> for Uuid {
    fn from(id: TaxTransactionId) -> Self {
        id.0
    }
}

impl AsRef<Uuid> for TaxTransactionId {
    fn as_ref(&self) -> &Uuid {
        &self.0
    }
}

impl std::ops::Deref for TaxTransactionId {
    type Target = Uuid;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct TaxTransaction {
    pub id: Uuid,
    pub company_id: Uuid,
    pub invoice_ref: Uuid,
    pub invoice_kind: InvoiceKind,
    pub posting_date: NaiveDate,
    pub taxable_base: Decimal,
    pub output_total: Decimal,
    pub input_total: Decimal,
    pub withholding_total: Decimal,
    pub currency: String,
    pub source: TaxTransactionSource,
    pub efaktur_document_id: Option<Uuid>,
    pub status: TaxTransactionStatus,
    #[serde(default)]
    #[sqlx(json)]
    pub metadata: AuditMetadata,
}

impl TaxTransaction {
    /// Create a builder for TaxTransaction
    pub fn builder() -> TaxTransactionBuilder {
        <TaxTransactionBuilder as Default>::default()
    }

    /// Create a new TaxTransaction with required fields
    pub fn new(
        company_id: Uuid,
        invoice_ref: Uuid,
        invoice_kind: InvoiceKind,
        posting_date: NaiveDate,
        taxable_base: Decimal,
        output_total: Decimal,
        input_total: Decimal,
        withholding_total: Decimal,
        currency: String,
        source: TaxTransactionSource,
        status: TaxTransactionStatus,
    ) -> Self {
        Self {
            id: Uuid::new_v4(),
            company_id,
            invoice_ref,
            invoice_kind,
            posting_date,
            taxable_base,
            output_total,
            input_total,
            withholding_total,
            currency,
            source,
            efaktur_document_id: None,
            status,
            metadata: AuditMetadata::default(),
        }
    }

    /// Get the entity's unique identifier
    pub fn id(&self) -> &Uuid {
        &self.id
    }

    /// Get a strongly-typed ID for this entity
    pub fn typed_id(&self) -> TaxTransactionId {
        TaxTransactionId(self.id)
    }

    /// Get when this entity was created
    pub fn created_at(&self) -> Option<&DateTime<Utc>> {
        self.metadata.created_at.as_ref()
    }

    /// Get when this entity was last updated
    pub fn updated_at(&self) -> Option<&DateTime<Utc>> {
        self.metadata.updated_at.as_ref()
    }

    /// Check if this entity is soft deleted
    pub fn is_deleted(&self) -> bool {
        self.metadata.deleted_at.is_some()
    }

    /// Check if this entity is active (not deleted)
    pub fn is_active(&self) -> bool {
        self.metadata.deleted_at.is_none()
    }

    /// Get when this entity was deleted
    pub fn deleted_at(&self) -> Option<&DateTime<Utc>> {
        self.metadata.deleted_at.as_ref()
    }

    /// Get who created this entity
    pub fn created_by(&self) -> Option<&Uuid> {
        self.metadata.created_by.as_ref()
    }

    /// Get who last updated this entity
    pub fn updated_by(&self) -> Option<&Uuid> {
        self.metadata.updated_by.as_ref()
    }

    /// Get who deleted this entity
    pub fn deleted_by(&self) -> Option<&Uuid> {
        self.metadata.deleted_by.as_ref()
    }

    /// Get the current status
    pub fn status(&self) -> &TaxTransactionStatus {
        &self.status
    }

    // ==========================================================
    // Fluent Setters (with_* for optional fields)
    // ==========================================================

    /// Set the efaktur_document_id field (chainable)
    pub fn with_efaktur_document_id(mut self, value: Uuid) -> Self {
        self.efaktur_document_id = Some(value);
        self
    }

    // ==========================================================
    // Partial Update
    // ==========================================================

    /// Apply partial updates from a map of field name to JSON value
    pub fn apply_patch(&mut self, fields: std::collections::HashMap<String, serde_json::Value>) {
        for (key, value) in fields {
            match key.as_str() {
                "company_id" => {
                    if let Ok(v) = serde_json::from_value(value) {
                        self.company_id = v;
                    }
                }
                "invoice_ref" => {
                    if let Ok(v) = serde_json::from_value(value) {
                        self.invoice_ref = v;
                    }
                }
                "invoice_kind" => {
                    if let Ok(v) = serde_json::from_value(value) {
                        self.invoice_kind = v;
                    }
                }
                "posting_date" => {
                    if let Ok(v) = serde_json::from_value(value) {
                        self.posting_date = v;
                    }
                }
                "taxable_base" => {
                    if let Ok(v) = serde_json::from_value(value) {
                        self.taxable_base = v;
                    }
                }
                "output_total" => {
                    if let Ok(v) = serde_json::from_value(value) {
                        self.output_total = v;
                    }
                }
                "input_total" => {
                    if let Ok(v) = serde_json::from_value(value) {
                        self.input_total = v;
                    }
                }
                "withholding_total" => {
                    if let Ok(v) = serde_json::from_value(value) {
                        self.withholding_total = v;
                    }
                }
                "currency" => {
                    if let Ok(v) = serde_json::from_value(value) {
                        self.currency = v;
                    }
                }
                "source" => {
                    if let Ok(v) = serde_json::from_value(value) {
                        self.source = v;
                    }
                }
                "efaktur_document_id" => {
                    if let Ok(v) = serde_json::from_value(value) {
                        self.efaktur_document_id = v;
                    }
                }
                "status" => {
                    if let Ok(v) = serde_json::from_value(value) {
                        self.status = v;
                    }
                }
                _ => {} // ignore unknown fields
            }
        }
    }

    // <<< CUSTOM METHODS START >>>
    // <<< CUSTOM METHODS END >>>
}

impl super::Entity for TaxTransaction {
    type Id = Uuid;

    fn entity_id(&self) -> &Self::Id {
        &self.id
    }

    fn entity_type() -> &'static str {
        "TaxTransaction"
    }
}

impl backbone_core::PersistentEntity for TaxTransaction {
    fn entity_id(&self) -> String {
        self.id.to_string()
    }
    fn set_entity_id(&mut self, id: String) {
        if let Ok(uuid) = uuid::Uuid::parse_str(&id) {
            self.id = uuid;
        }
    }
    fn created_at(&self) -> Option<chrono::DateTime<chrono::Utc>> {
        self.metadata.created_at
    }
    fn set_created_at(&mut self, ts: chrono::DateTime<chrono::Utc>) {
        self.metadata.created_at = Some(ts);
    }
    fn updated_at(&self) -> Option<chrono::DateTime<chrono::Utc>> {
        self.metadata.updated_at
    }
    fn set_updated_at(&mut self, ts: chrono::DateTime<chrono::Utc>) {
        self.metadata.updated_at = Some(ts);
    }
    fn deleted_at(&self) -> Option<chrono::DateTime<chrono::Utc>> {
        self.metadata.deleted_at
    }
    fn set_deleted_at(&mut self, ts: Option<chrono::DateTime<chrono::Utc>>) {
        self.metadata.deleted_at = ts;
    }
}

impl backbone_orm::EntityRepoMeta for TaxTransaction {
    fn column_types() -> std::collections::HashMap<String, String> {
        let mut m = std::collections::HashMap::new();
        m.insert("id".to_string(), "uuid".to_string());
        m.insert("company_id".to_string(), "uuid".to_string());
        m.insert("efaktur_document_id".to_string(), "uuid".to_string());
        m.insert("invoice_kind".to_string(), "invoice_kind".to_string());
        m.insert("source".to_string(), "tax_transaction_source".to_string());
        m.insert("status".to_string(), "tax_transaction_status".to_string());
        m
    }
    fn search_fields() -> &'static [&'static str] {
        &["currency"]
    }
    fn company_field() -> Option<&'static str> {
        Some("company_id")
    }
}

/// Builder for TaxTransaction entity
///
/// Provides a fluent API for constructing TaxTransaction instances.
/// System fields (id, metadata, timestamps) are auto-initialized.
#[derive(Debug, Clone, Default)]
pub struct TaxTransactionBuilder {
    company_id: Option<Uuid>,
    invoice_ref: Option<Uuid>,
    invoice_kind: Option<InvoiceKind>,
    posting_date: Option<NaiveDate>,
    taxable_base: Option<Decimal>,
    output_total: Option<Decimal>,
    input_total: Option<Decimal>,
    withholding_total: Option<Decimal>,
    currency: Option<String>,
    source: Option<TaxTransactionSource>,
    efaktur_document_id: Option<Uuid>,
    status: Option<TaxTransactionStatus>,
}

impl TaxTransactionBuilder {
    /// Set the company_id field (required)
    pub fn company_id(mut self, value: Uuid) -> Self {
        self.company_id = Some(value);
        self
    }

    /// Set the invoice_ref field (required)
    pub fn invoice_ref(mut self, value: Uuid) -> Self {
        self.invoice_ref = Some(value);
        self
    }

    /// Set the invoice_kind field (required)
    pub fn invoice_kind(mut self, value: InvoiceKind) -> Self {
        self.invoice_kind = Some(value);
        self
    }

    /// Set the posting_date field (required)
    pub fn posting_date(mut self, value: NaiveDate) -> Self {
        self.posting_date = Some(value);
        self
    }

    /// Set the taxable_base field (required)
    pub fn taxable_base(mut self, value: Decimal) -> Self {
        self.taxable_base = Some(value);
        self
    }

    /// Set the output_total field (default: `Decimal::from(0)`)
    pub fn output_total(mut self, value: Decimal) -> Self {
        self.output_total = Some(value);
        self
    }

    /// Set the input_total field (default: `Decimal::from(0)`)
    pub fn input_total(mut self, value: Decimal) -> Self {
        self.input_total = Some(value);
        self
    }

    /// Set the withholding_total field (default: `Decimal::from(0)`)
    pub fn withholding_total(mut self, value: Decimal) -> Self {
        self.withholding_total = Some(value);
        self
    }

    /// Set the currency field (default: `Default::default()`)
    pub fn currency(mut self, value: String) -> Self {
        self.currency = Some(value);
        self
    }

    /// Set the source field (default: `TaxTransactionSource::default()`)
    pub fn source(mut self, value: TaxTransactionSource) -> Self {
        self.source = Some(value);
        self
    }

    /// Set the efaktur_document_id field (optional)
    pub fn efaktur_document_id(mut self, value: Uuid) -> Self {
        self.efaktur_document_id = Some(value);
        self
    }

    /// Set the status field (default: `TaxTransactionStatus::default()`)
    pub fn status(mut self, value: TaxTransactionStatus) -> Self {
        self.status = Some(value);
        self
    }

    /// Build the TaxTransaction entity
    ///
    /// Returns Err if any required field without a default is missing.
    pub fn build(self) -> Result<TaxTransaction, String> {
        let company_id = self
            .company_id
            .ok_or_else(|| "company_id is required".to_string())?;
        let invoice_ref = self
            .invoice_ref
            .ok_or_else(|| "invoice_ref is required".to_string())?;
        let invoice_kind = self
            .invoice_kind
            .ok_or_else(|| "invoice_kind is required".to_string())?;
        let posting_date = self
            .posting_date
            .ok_or_else(|| "posting_date is required".to_string())?;
        let taxable_base = self
            .taxable_base
            .ok_or_else(|| "taxable_base is required".to_string())?;

        Ok(TaxTransaction {
            id: Uuid::new_v4(),
            company_id,
            invoice_ref,
            invoice_kind,
            posting_date,
            taxable_base,
            output_total: self.output_total.unwrap_or(Decimal::from(0)),
            input_total: self.input_total.unwrap_or(Decimal::from(0)),
            withholding_total: self.withholding_total.unwrap_or(Decimal::from(0)),
            currency: self.currency.unwrap_or_default(),
            source: self.source.unwrap_or_default(),
            efaktur_document_id: self.efaktur_document_id,
            status: self.status.unwrap_or_default(),
            metadata: AuditMetadata::default(),
        })
    }
}
