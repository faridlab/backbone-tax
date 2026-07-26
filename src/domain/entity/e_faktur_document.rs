use chrono::{DateTime, Utc, NaiveDate};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

use super::EFakturStatus;
use super::AuditMetadata;

/// Strongly-typed ID for EFakturDocument
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct EFakturDocumentId(pub Uuid);

impl EFakturDocumentId {
    pub fn new(id: Uuid) -> Self { Self(id) }
    pub fn generate() -> Self { Self(Uuid::new_v4()) }
    pub fn into_inner(self) -> Uuid { self.0 }
}

impl std::fmt::Display for EFakturDocumentId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::str::FromStr for EFakturDocumentId {
    type Err = uuid::Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self(Uuid::parse_str(s)?))
    }
}

impl From<Uuid> for EFakturDocumentId {
    fn from(id: Uuid) -> Self { Self(id) }
}

impl From<EFakturDocumentId> for Uuid {
    fn from(id: EFakturDocumentId) -> Self { id.0 }
}

impl AsRef<Uuid> for EFakturDocumentId {
    fn as_ref(&self) -> &Uuid { &self.0 }
}

impl std::ops::Deref for EFakturDocumentId {
    type Target = Uuid;
    fn deref(&self) -> &Self::Target { &self.0 }
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct EFakturDocument {
    pub id: Uuid,
    pub company_id: Uuid,
    pub tax_transaction_id: Uuid,
    pub number: String,
    pub transaction_code: String,
    pub taxpayer_segment: String,
    pub period: NaiveDate,
    pub sequence: i32,
    pub assignment_date: NaiveDate,
    pub status: EFakturStatus,
    pub replaces_id: Option<Uuid>,
    #[serde(default)]
    #[sqlx(json)]
    pub metadata: AuditMetadata,
}

impl EFakturDocument {
    /// Create a builder for EFakturDocument
    pub fn builder() -> EFakturDocumentBuilder {
        EFakturDocumentBuilder::default()
    }

    /// Create a new EFakturDocument with required fields
    pub fn new(company_id: Uuid, tax_transaction_id: Uuid, number: String, transaction_code: String, taxpayer_segment: String, period: NaiveDate, sequence: i32, assignment_date: NaiveDate, status: EFakturStatus) -> Self {
        Self {
            id: Uuid::new_v4(),
            company_id,
            tax_transaction_id,
            number,
            transaction_code,
            taxpayer_segment,
            period,
            sequence,
            assignment_date,
            status,
            replaces_id: None,
            metadata: AuditMetadata::default(),
        }
    }

    /// Get the entity's unique identifier
    pub fn id(&self) -> &Uuid {
        &self.id
    }

    /// Get a strongly-typed ID for this entity
    pub fn typed_id(&self) -> EFakturDocumentId {
        EFakturDocumentId(self.id)
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
    pub fn status(&self) -> &EFakturStatus {
        &self.status
    }


    // ==========================================================
    // Fluent Setters (with_* for optional fields)
    // ==========================================================

    /// Set the replaces_id field (chainable)
    pub fn with_replaces_id(mut self, value: Uuid) -> Self {
        self.replaces_id = Some(value);
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
                    if let Ok(v) = serde_json::from_value(value) { self.company_id = v; }
                }
                "tax_transaction_id" => {
                    if let Ok(v) = serde_json::from_value(value) { self.tax_transaction_id = v; }
                }
                "number" => {
                    if let Ok(v) = serde_json::from_value(value) { self.number = v; }
                }
                "transaction_code" => {
                    if let Ok(v) = serde_json::from_value(value) { self.transaction_code = v; }
                }
                "taxpayer_segment" => {
                    if let Ok(v) = serde_json::from_value(value) { self.taxpayer_segment = v; }
                }
                "period" => {
                    if let Ok(v) = serde_json::from_value(value) { self.period = v; }
                }
                "sequence" => {
                    if let Ok(v) = serde_json::from_value(value) { self.sequence = v; }
                }
                "assignment_date" => {
                    if let Ok(v) = serde_json::from_value(value) { self.assignment_date = v; }
                }
                "status" => {
                    if let Ok(v) = serde_json::from_value(value) { self.status = v; }
                }
                "replaces_id" => {
                    if let Ok(v) = serde_json::from_value(value) { self.replaces_id = v; }
                }
                _ => {} // ignore unknown fields
            }
        }
    }

    // <<< CUSTOM METHODS START >>>
    // <<< CUSTOM METHODS END >>>
}

impl super::Entity for EFakturDocument {
    type Id = Uuid;

    fn entity_id(&self) -> &Self::Id {
        &self.id
    }

    fn entity_type() -> &'static str {
        "EFakturDocument"
    }
}

impl backbone_core::PersistentEntity for EFakturDocument {
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

impl backbone_orm::EntityRepoMeta for EFakturDocument {
    fn column_types() -> std::collections::HashMap<String, String> {
        let mut m = std::collections::HashMap::new();
        m.insert("id".to_string(), "uuid".to_string());
        m.insert("company_id".to_string(), "uuid".to_string());
        m.insert("tax_transaction_id".to_string(), "uuid".to_string());
        m.insert("replaces_id".to_string(), "uuid".to_string());
        m.insert("status".to_string(), "e_faktur_status".to_string());
        m
    }
    fn search_fields() -> &'static [&'static str] {
        &["number", "transaction_code", "taxpayer_segment"]
    }
    fn company_field() -> Option<&'static str> {
        Some("company_id")
    }
}

/// Builder for EFakturDocument entity
///
/// Provides a fluent API for constructing EFakturDocument instances.
/// System fields (id, metadata, timestamps) are auto-initialized.
#[derive(Debug, Clone, Default)]
pub struct EFakturDocumentBuilder {
    company_id: Option<Uuid>,
    tax_transaction_id: Option<Uuid>,
    number: Option<String>,
    transaction_code: Option<String>,
    taxpayer_segment: Option<String>,
    period: Option<NaiveDate>,
    sequence: Option<i32>,
    assignment_date: Option<NaiveDate>,
    status: Option<EFakturStatus>,
    replaces_id: Option<Uuid>,
}

impl EFakturDocumentBuilder {
    /// Set the company_id field (required)
    pub fn company_id(mut self, value: Uuid) -> Self {
        self.company_id = Some(value);
        self
    }

    /// Set the tax_transaction_id field (required)
    pub fn tax_transaction_id(mut self, value: Uuid) -> Self {
        self.tax_transaction_id = Some(value);
        self
    }

    /// Set the number field (required)
    pub fn number(mut self, value: String) -> Self {
        self.number = Some(value);
        self
    }

    /// Set the transaction_code field (required)
    pub fn transaction_code(mut self, value: String) -> Self {
        self.transaction_code = Some(value);
        self
    }

    /// Set the taxpayer_segment field (required)
    pub fn taxpayer_segment(mut self, value: String) -> Self {
        self.taxpayer_segment = Some(value);
        self
    }

    /// Set the period field (required)
    pub fn period(mut self, value: NaiveDate) -> Self {
        self.period = Some(value);
        self
    }

    /// Set the sequence field (required)
    pub fn sequence(mut self, value: i32) -> Self {
        self.sequence = Some(value);
        self
    }

    /// Set the assignment_date field (required)
    pub fn assignment_date(mut self, value: NaiveDate) -> Self {
        self.assignment_date = Some(value);
        self
    }

    /// Set the status field (default: `EFakturStatus::default()`)
    pub fn status(mut self, value: EFakturStatus) -> Self {
        self.status = Some(value);
        self
    }

    /// Set the replaces_id field (optional)
    pub fn replaces_id(mut self, value: Uuid) -> Self {
        self.replaces_id = Some(value);
        self
    }

    /// Build the EFakturDocument entity
    ///
    /// Returns Err if any required field without a default is missing.
    pub fn build(self) -> Result<EFakturDocument, String> {
        let company_id = self.company_id.ok_or_else(|| "company_id is required".to_string())?;
        let tax_transaction_id = self.tax_transaction_id.ok_or_else(|| "tax_transaction_id is required".to_string())?;
        let number = self.number.ok_or_else(|| "number is required".to_string())?;
        let transaction_code = self.transaction_code.ok_or_else(|| "transaction_code is required".to_string())?;
        let taxpayer_segment = self.taxpayer_segment.ok_or_else(|| "taxpayer_segment is required".to_string())?;
        let period = self.period.ok_or_else(|| "period is required".to_string())?;
        let sequence = self.sequence.ok_or_else(|| "sequence is required".to_string())?;
        let assignment_date = self.assignment_date.ok_or_else(|| "assignment_date is required".to_string())?;

        Ok(EFakturDocument {
            id: Uuid::new_v4(),
            company_id,
            tax_transaction_id,
            number,
            transaction_code,
            taxpayer_segment,
            period,
            sequence,
            assignment_date,
            status: self.status.unwrap_or(EFakturStatus::default()),
            replaces_id: self.replaces_id,
            metadata: AuditMetadata::default(),
        })
    }
}
