use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

use super::AuditMetadata;
use super::RepartitionDocumentType;
use super::RepartitionType;

/// Strongly-typed ID for TaxRepartitionLine
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct TaxRepartitionLineId(pub Uuid);

impl TaxRepartitionLineId {
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

impl std::fmt::Display for TaxRepartitionLineId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::str::FromStr for TaxRepartitionLineId {
    type Err = uuid::Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self(Uuid::parse_str(s)?))
    }
}

impl From<Uuid> for TaxRepartitionLineId {
    fn from(id: Uuid) -> Self {
        Self(id)
    }
}

impl From<TaxRepartitionLineId> for Uuid {
    fn from(id: TaxRepartitionLineId) -> Self {
        id.0
    }
}

impl AsRef<Uuid> for TaxRepartitionLineId {
    fn as_ref(&self) -> &Uuid {
        &self.0
    }
}

impl std::ops::Deref for TaxRepartitionLineId {
    type Target = Uuid;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct TaxRepartitionLine {
    pub id: Uuid,
    pub company_id: Uuid,
    pub template_id: Uuid,
    pub document_type: RepartitionDocumentType,
    pub repartition_type: RepartitionType,
    pub factor_percent: Decimal,
    pub account_id: Option<Uuid>,
    pub tag_ids: Vec<Uuid>,
    pub sort_order: i32,
    pub description: Option<String>,
    #[serde(default)]
    #[sqlx(json)]
    pub metadata: AuditMetadata,
}

impl TaxRepartitionLine {
    /// Create a builder for TaxRepartitionLine
    pub fn builder() -> TaxRepartitionLineBuilder {
        <TaxRepartitionLineBuilder as Default>::default()
    }

    /// Create a new TaxRepartitionLine with required fields
    pub fn new(
        company_id: Uuid,
        template_id: Uuid,
        document_type: RepartitionDocumentType,
        repartition_type: RepartitionType,
        factor_percent: Decimal,
        tag_ids: Vec<Uuid>,
        sort_order: i32,
    ) -> Self {
        Self {
            id: Uuid::new_v4(),
            company_id,
            template_id,
            document_type,
            repartition_type,
            factor_percent,
            account_id: None,
            tag_ids,
            sort_order,
            description: None,
            metadata: AuditMetadata::default(),
        }
    }

    /// Get the entity's unique identifier
    pub fn id(&self) -> &Uuid {
        &self.id
    }

    /// Get a strongly-typed ID for this entity
    pub fn typed_id(&self) -> TaxRepartitionLineId {
        TaxRepartitionLineId(self.id)
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

    // ==========================================================
    // Fluent Setters (with_* for optional fields)
    // ==========================================================

    /// Set the account_id field (chainable)
    pub fn with_account_id(mut self, value: Uuid) -> Self {
        self.account_id = Some(value);
        self
    }

    /// Set the description field (chainable)
    pub fn with_description(mut self, value: String) -> Self {
        self.description = Some(value);
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
                "template_id" => {
                    if let Ok(v) = serde_json::from_value(value) {
                        self.template_id = v;
                    }
                }
                "document_type" => {
                    if let Ok(v) = serde_json::from_value(value) {
                        self.document_type = v;
                    }
                }
                "repartition_type" => {
                    if let Ok(v) = serde_json::from_value(value) {
                        self.repartition_type = v;
                    }
                }
                "factor_percent" => {
                    if let Ok(v) = serde_json::from_value(value) {
                        self.factor_percent = v;
                    }
                }
                "account_id" => {
                    if let Ok(v) = serde_json::from_value(value) {
                        self.account_id = v;
                    }
                }
                "tag_ids" => {
                    if let Ok(v) = serde_json::from_value(value) {
                        self.tag_ids = v;
                    }
                }
                "sort_order" => {
                    if let Ok(v) = serde_json::from_value(value) {
                        self.sort_order = v;
                    }
                }
                "description" => {
                    if let Ok(v) = serde_json::from_value(value) {
                        self.description = v;
                    }
                }
                _ => {} // ignore unknown fields
            }
        }
    }

    // <<< CUSTOM METHODS START >>>
    // <<< CUSTOM METHODS END >>>
}

impl super::Entity for TaxRepartitionLine {
    type Id = Uuid;

    fn entity_id(&self) -> &Self::Id {
        &self.id
    }

    fn entity_type() -> &'static str {
        "TaxRepartitionLine"
    }
}

impl backbone_core::PersistentEntity for TaxRepartitionLine {
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

impl backbone_orm::EntityRepoMeta for TaxRepartitionLine {
    fn column_types() -> std::collections::HashMap<String, String> {
        let mut m = std::collections::HashMap::new();
        m.insert("id".to_string(), "uuid".to_string());
        m.insert("company_id".to_string(), "uuid".to_string());
        m.insert("template_id".to_string(), "uuid".to_string());
        m.insert("account_id".to_string(), "uuid".to_string());
        m.insert(
            "document_type".to_string(),
            "repartition_document_type".to_string(),
        );
        m.insert(
            "repartition_type".to_string(),
            "repartition_type".to_string(),
        );
        m
    }
    fn search_fields() -> &'static [&'static str] {
        &[]
    }
    fn company_field() -> Option<&'static str> {
        Some("company_id")
    }
    fn relations() -> &'static [(&'static str, &'static str, &'static str)] {
        &[("template", "tax_templates", "templateId")]
    }
}

/// Builder for TaxRepartitionLine entity
///
/// Provides a fluent API for constructing TaxRepartitionLine instances.
/// System fields (id, metadata, timestamps) are auto-initialized.
#[derive(Debug, Clone, Default)]
pub struct TaxRepartitionLineBuilder {
    company_id: Option<Uuid>,
    template_id: Option<Uuid>,
    document_type: Option<RepartitionDocumentType>,
    repartition_type: Option<RepartitionType>,
    factor_percent: Option<Decimal>,
    account_id: Option<Uuid>,
    tag_ids: Option<Vec<Uuid>>,
    sort_order: Option<i32>,
    description: Option<String>,
}

impl TaxRepartitionLineBuilder {
    /// Set the company_id field (required)
    pub fn company_id(mut self, value: Uuid) -> Self {
        self.company_id = Some(value);
        self
    }

    /// Set the template_id field (required)
    pub fn template_id(mut self, value: Uuid) -> Self {
        self.template_id = Some(value);
        self
    }

    /// Set the document_type field (default: `RepartitionDocumentType::default()`)
    pub fn document_type(mut self, value: RepartitionDocumentType) -> Self {
        self.document_type = Some(value);
        self
    }

    /// Set the repartition_type field (default: `RepartitionType::default()`)
    pub fn repartition_type(mut self, value: RepartitionType) -> Self {
        self.repartition_type = Some(value);
        self
    }

    /// Set the factor_percent field (required)
    pub fn factor_percent(mut self, value: Decimal) -> Self {
        self.factor_percent = Some(value);
        self
    }

    /// Set the account_id field (optional)
    pub fn account_id(mut self, value: Uuid) -> Self {
        self.account_id = Some(value);
        self
    }

    /// Set the tag_ids field (required)
    pub fn tag_ids(mut self, value: Vec<Uuid>) -> Self {
        self.tag_ids = Some(value);
        self
    }

    /// Set the sort_order field (default: `0`)
    pub fn sort_order(mut self, value: i32) -> Self {
        self.sort_order = Some(value);
        self
    }

    /// Set the description field (optional)
    pub fn description(mut self, value: String) -> Self {
        self.description = Some(value);
        self
    }

    /// Build the TaxRepartitionLine entity
    ///
    /// Returns Err if any required field without a default is missing.
    pub fn build(self) -> Result<TaxRepartitionLine, String> {
        let company_id = self
            .company_id
            .ok_or_else(|| "company_id is required".to_string())?;
        let template_id = self
            .template_id
            .ok_or_else(|| "template_id is required".to_string())?;
        let factor_percent = self
            .factor_percent
            .ok_or_else(|| "factor_percent is required".to_string())?;
        let tag_ids = self
            .tag_ids
            .ok_or_else(|| "tag_ids is required".to_string())?;

        Ok(TaxRepartitionLine {
            id: Uuid::new_v4(),
            company_id,
            template_id,
            document_type: self.document_type.unwrap_or_default(),
            repartition_type: self.repartition_type.unwrap_or_default(),
            factor_percent,
            account_id: self.account_id,
            tag_ids,
            sort_order: self.sort_order.unwrap_or(0),
            description: self.description,
            metadata: AuditMetadata::default(),
        })
    }
}
