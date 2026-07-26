use chrono::{DateTime, Utc, NaiveDate};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;
use rust_decimal::Decimal;

use super::TaxFilingStatus;
use super::AuditMetadata;

/// Strongly-typed ID for TaxFilingPeriod
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct TaxFilingPeriodId(pub Uuid);

impl TaxFilingPeriodId {
    pub fn new(id: Uuid) -> Self { Self(id) }
    pub fn generate() -> Self { Self(Uuid::new_v4()) }
    pub fn into_inner(self) -> Uuid { self.0 }
}

impl std::fmt::Display for TaxFilingPeriodId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::str::FromStr for TaxFilingPeriodId {
    type Err = uuid::Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self(Uuid::parse_str(s)?))
    }
}

impl From<Uuid> for TaxFilingPeriodId {
    fn from(id: Uuid) -> Self { Self(id) }
}

impl From<TaxFilingPeriodId> for Uuid {
    fn from(id: TaxFilingPeriodId) -> Self { id.0 }
}

impl AsRef<Uuid> for TaxFilingPeriodId {
    fn as_ref(&self) -> &Uuid { &self.0 }
}

impl std::ops::Deref for TaxFilingPeriodId {
    type Target = Uuid;
    fn deref(&self) -> &Self::Target { &self.0 }
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct TaxFilingPeriod {
    pub id: Uuid,
    pub company_id: Uuid,
    pub period: NaiveDate,
    pub npwp: Option<String>,
    pub taxpayer_segment: Option<String>,
    pub next_sequence: i32,
    pub output_total: Decimal,
    pub input_total: Decimal,
    pub withholding_total: Decimal,
    pub status: TaxFilingStatus,
    #[serde(default)]
    #[sqlx(json)]
    pub metadata: AuditMetadata,
}

impl TaxFilingPeriod {
    /// Create a builder for TaxFilingPeriod
    pub fn builder() -> TaxFilingPeriodBuilder {
        TaxFilingPeriodBuilder::default()
    }

    /// Create a new TaxFilingPeriod with required fields
    pub fn new(company_id: Uuid, period: NaiveDate, next_sequence: i32, output_total: Decimal, input_total: Decimal, withholding_total: Decimal, status: TaxFilingStatus) -> Self {
        Self {
            id: Uuid::new_v4(),
            company_id,
            period,
            npwp: None,
            taxpayer_segment: None,
            next_sequence,
            output_total,
            input_total,
            withholding_total,
            status,
            metadata: AuditMetadata::default(),
        }
    }

    /// Get the entity's unique identifier
    pub fn id(&self) -> &Uuid {
        &self.id
    }

    /// Get a strongly-typed ID for this entity
    pub fn typed_id(&self) -> TaxFilingPeriodId {
        TaxFilingPeriodId(self.id)
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
    pub fn status(&self) -> &TaxFilingStatus {
        &self.status
    }


    // ==========================================================
    // Fluent Setters (with_* for optional fields)
    // ==========================================================

    /// Set the npwp field (chainable)
    pub fn with_npwp(mut self, value: String) -> Self {
        self.npwp = Some(value);
        self
    }

    /// Set the taxpayer_segment field (chainable)
    pub fn with_taxpayer_segment(mut self, value: String) -> Self {
        self.taxpayer_segment = Some(value);
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
                "period" => {
                    if let Ok(v) = serde_json::from_value(value) { self.period = v; }
                }
                "npwp" => {
                    if let Ok(v) = serde_json::from_value(value) { self.npwp = v; }
                }
                "taxpayer_segment" => {
                    if let Ok(v) = serde_json::from_value(value) { self.taxpayer_segment = v; }
                }
                "next_sequence" => {
                    if let Ok(v) = serde_json::from_value(value) { self.next_sequence = v; }
                }
                "output_total" => {
                    if let Ok(v) = serde_json::from_value(value) { self.output_total = v; }
                }
                "input_total" => {
                    if let Ok(v) = serde_json::from_value(value) { self.input_total = v; }
                }
                "withholding_total" => {
                    if let Ok(v) = serde_json::from_value(value) { self.withholding_total = v; }
                }
                "status" => {
                    if let Ok(v) = serde_json::from_value(value) { self.status = v; }
                }
                _ => {} // ignore unknown fields
            }
        }
    }

    // <<< CUSTOM METHODS START >>>
    // <<< CUSTOM METHODS END >>>
}

impl super::Entity for TaxFilingPeriod {
    type Id = Uuid;

    fn entity_id(&self) -> &Self::Id {
        &self.id
    }

    fn entity_type() -> &'static str {
        "TaxFilingPeriod"
    }
}

impl backbone_core::PersistentEntity for TaxFilingPeriod {
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

impl backbone_orm::EntityRepoMeta for TaxFilingPeriod {
    fn column_types() -> std::collections::HashMap<String, String> {
        let mut m = std::collections::HashMap::new();
        m.insert("id".to_string(), "uuid".to_string());
        m.insert("company_id".to_string(), "uuid".to_string());
        m.insert("status".to_string(), "tax_filing_status".to_string());
        m
    }
    fn search_fields() -> &'static [&'static str] {
        &[]
    }
    fn company_field() -> Option<&'static str> {
        Some("company_id")
    }
}

/// Builder for TaxFilingPeriod entity
///
/// Provides a fluent API for constructing TaxFilingPeriod instances.
/// System fields (id, metadata, timestamps) are auto-initialized.
#[derive(Debug, Clone, Default)]
pub struct TaxFilingPeriodBuilder {
    company_id: Option<Uuid>,
    period: Option<NaiveDate>,
    npwp: Option<String>,
    taxpayer_segment: Option<String>,
    next_sequence: Option<i32>,
    output_total: Option<Decimal>,
    input_total: Option<Decimal>,
    withholding_total: Option<Decimal>,
    status: Option<TaxFilingStatus>,
}

impl TaxFilingPeriodBuilder {
    /// Set the company_id field (required)
    pub fn company_id(mut self, value: Uuid) -> Self {
        self.company_id = Some(value);
        self
    }

    /// Set the period field (required)
    pub fn period(mut self, value: NaiveDate) -> Self {
        self.period = Some(value);
        self
    }

    /// Set the npwp field (optional)
    pub fn npwp(mut self, value: String) -> Self {
        self.npwp = Some(value);
        self
    }

    /// Set the taxpayer_segment field (optional)
    pub fn taxpayer_segment(mut self, value: String) -> Self {
        self.taxpayer_segment = Some(value);
        self
    }

    /// Set the next_sequence field (default: `1`)
    pub fn next_sequence(mut self, value: i32) -> Self {
        self.next_sequence = Some(value);
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

    /// Set the status field (default: `TaxFilingStatus::default()`)
    pub fn status(mut self, value: TaxFilingStatus) -> Self {
        self.status = Some(value);
        self
    }

    /// Build the TaxFilingPeriod entity
    ///
    /// Returns Err if any required field without a default is missing.
    pub fn build(self) -> Result<TaxFilingPeriod, String> {
        let company_id = self.company_id.ok_or_else(|| "company_id is required".to_string())?;
        let period = self.period.ok_or_else(|| "period is required".to_string())?;

        Ok(TaxFilingPeriod {
            id: Uuid::new_v4(),
            company_id,
            period,
            npwp: self.npwp,
            taxpayer_segment: self.taxpayer_segment,
            next_sequence: self.next_sequence.unwrap_or(1),
            output_total: self.output_total.unwrap_or(Decimal::from(0)),
            input_total: self.input_total.unwrap_or(Decimal::from(0)),
            withholding_total: self.withholding_total.unwrap_or(Decimal::from(0)),
            status: self.status.unwrap_or(TaxFilingStatus::default()),
            metadata: AuditMetadata::default(),
        })
    }
}
