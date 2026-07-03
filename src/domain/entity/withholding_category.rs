use chrono::{DateTime, Utc, NaiveDate};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;
use rust_decimal::Decimal;

use super::TaxStatus;
use super::AuditMetadata;

/// Strongly-typed ID for WithholdingCategory
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct WithholdingCategoryId(pub Uuid);

impl WithholdingCategoryId {
    pub fn new(id: Uuid) -> Self { Self(id) }
    pub fn generate() -> Self { Self(Uuid::new_v4()) }
    pub fn into_inner(self) -> Uuid { self.0 }
}

impl std::fmt::Display for WithholdingCategoryId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::str::FromStr for WithholdingCategoryId {
    type Err = uuid::Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self(Uuid::parse_str(s)?))
    }
}

impl From<Uuid> for WithholdingCategoryId {
    fn from(id: Uuid) -> Self { Self(id) }
}

impl From<WithholdingCategoryId> for Uuid {
    fn from(id: WithholdingCategoryId) -> Self { id.0 }
}

impl AsRef<Uuid> for WithholdingCategoryId {
    fn as_ref(&self) -> &Uuid { &self.0 }
}

impl std::ops::Deref for WithholdingCategoryId {
    type Target = Uuid;
    fn deref(&self) -> &Self::Target { &self.0 }
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct WithholdingCategory {
    pub id: Uuid,
    pub code: String,
    pub name: String,
    pub rate: Decimal,
    pub threshold_amount: Decimal,
    pub account_id: Option<Uuid>,
    pub effective_from: NaiveDate,
    pub effective_to: Option<NaiveDate>,
    pub status: TaxStatus,
    #[serde(default)]
    #[sqlx(json)]
    pub metadata: AuditMetadata,
}

impl WithholdingCategory {
    /// Create a builder for WithholdingCategory
    pub fn builder() -> WithholdingCategoryBuilder {
        WithholdingCategoryBuilder::default()
    }

    /// Create a new WithholdingCategory with required fields
    pub fn new(code: String, name: String, rate: Decimal, threshold_amount: Decimal, effective_from: NaiveDate, status: TaxStatus) -> Self {
        Self {
            id: Uuid::new_v4(),
            code,
            name,
            rate,
            threshold_amount,
            account_id: None,
            effective_from,
            effective_to: None,
            status,
            metadata: AuditMetadata::default(),
        }
    }

    /// Get the entity's unique identifier
    pub fn id(&self) -> &Uuid {
        &self.id
    }

    /// Get a strongly-typed ID for this entity
    pub fn typed_id(&self) -> WithholdingCategoryId {
        WithholdingCategoryId(self.id)
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
    pub fn status(&self) -> &TaxStatus {
        &self.status
    }


    // ==========================================================
    // Fluent Setters (with_* for optional fields)
    // ==========================================================

    /// Set the account_id field (chainable)
    pub fn with_account_id(mut self, value: Uuid) -> Self {
        self.account_id = Some(value);
        self
    }

    /// Set the effective_to field (chainable)
    pub fn with_effective_to(mut self, value: NaiveDate) -> Self {
        self.effective_to = Some(value);
        self
    }

    // ==========================================================
    // Partial Update
    // ==========================================================

    /// Apply partial updates from a map of field name to JSON value
    pub fn apply_patch(&mut self, fields: std::collections::HashMap<String, serde_json::Value>) {
        for (key, value) in fields {
            match key.as_str() {
                "code" => {
                    if let Ok(v) = serde_json::from_value(value) { self.code = v; }
                }
                "name" => {
                    if let Ok(v) = serde_json::from_value(value) { self.name = v; }
                }
                "rate" => {
                    if let Ok(v) = serde_json::from_value(value) { self.rate = v; }
                }
                "threshold_amount" => {
                    if let Ok(v) = serde_json::from_value(value) { self.threshold_amount = v; }
                }
                "account_id" => {
                    if let Ok(v) = serde_json::from_value(value) { self.account_id = v; }
                }
                "effective_from" => {
                    if let Ok(v) = serde_json::from_value(value) { self.effective_from = v; }
                }
                "effective_to" => {
                    if let Ok(v) = serde_json::from_value(value) { self.effective_to = v; }
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

impl super::Entity for WithholdingCategory {
    type Id = Uuid;

    fn entity_id(&self) -> &Self::Id {
        &self.id
    }

    fn entity_type() -> &'static str {
        "WithholdingCategory"
    }
}

impl backbone_core::PersistentEntity for WithholdingCategory {
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

impl backbone_orm::EntityRepoMeta for WithholdingCategory {
    fn column_types() -> std::collections::HashMap<String, String> {
        let mut m = std::collections::HashMap::new();
        m.insert("id".to_string(), "uuid".to_string());
        m.insert("account_id".to_string(), "uuid".to_string());
        m.insert("status".to_string(), "tax_status".to_string());
        m
    }
    fn search_fields() -> &'static [&'static str] {
        &["code", "name"]
    }
}

/// Builder for WithholdingCategory entity
///
/// Provides a fluent API for constructing WithholdingCategory instances.
/// System fields (id, metadata, timestamps) are auto-initialized.
#[derive(Debug, Clone, Default)]
pub struct WithholdingCategoryBuilder {
    code: Option<String>,
    name: Option<String>,
    rate: Option<Decimal>,
    threshold_amount: Option<Decimal>,
    account_id: Option<Uuid>,
    effective_from: Option<NaiveDate>,
    effective_to: Option<NaiveDate>,
    status: Option<TaxStatus>,
}

impl WithholdingCategoryBuilder {
    /// Set the code field (required)
    pub fn code(mut self, value: String) -> Self {
        self.code = Some(value);
        self
    }

    /// Set the name field (required)
    pub fn name(mut self, value: String) -> Self {
        self.name = Some(value);
        self
    }

    /// Set the rate field (required)
    pub fn rate(mut self, value: Decimal) -> Self {
        self.rate = Some(value);
        self
    }

    /// Set the threshold_amount field (default: `Decimal::from(0)`)
    pub fn threshold_amount(mut self, value: Decimal) -> Self {
        self.threshold_amount = Some(value);
        self
    }

    /// Set the account_id field (optional)
    pub fn account_id(mut self, value: Uuid) -> Self {
        self.account_id = Some(value);
        self
    }

    /// Set the effective_from field (required)
    pub fn effective_from(mut self, value: NaiveDate) -> Self {
        self.effective_from = Some(value);
        self
    }

    /// Set the effective_to field (optional)
    pub fn effective_to(mut self, value: NaiveDate) -> Self {
        self.effective_to = Some(value);
        self
    }

    /// Set the status field (default: `TaxStatus::default()`)
    pub fn status(mut self, value: TaxStatus) -> Self {
        self.status = Some(value);
        self
    }

    /// Build the WithholdingCategory entity
    ///
    /// Returns Err if any required field without a default is missing.
    pub fn build(self) -> Result<WithholdingCategory, String> {
        let code = self.code.ok_or_else(|| "code is required".to_string())?;
        let name = self.name.ok_or_else(|| "name is required".to_string())?;
        let rate = self.rate.ok_or_else(|| "rate is required".to_string())?;
        let effective_from = self.effective_from.ok_or_else(|| "effective_from is required".to_string())?;

        Ok(WithholdingCategory {
            id: Uuid::new_v4(),
            code,
            name,
            rate,
            threshold_amount: self.threshold_amount.unwrap_or(Decimal::from(0)),
            account_id: self.account_id,
            effective_from,
            effective_to: self.effective_to,
            status: self.status.unwrap_or(TaxStatus::default()),
            metadata: AuditMetadata::default(),
        })
    }
}
