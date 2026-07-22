use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

use super::TemplateType;
use super::TaxStatus;
use super::AuditMetadata;

/// Strongly-typed ID for TaxTemplate
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct TaxTemplateId(pub Uuid);

impl TaxTemplateId {
    pub fn new(id: Uuid) -> Self { Self(id) }
    pub fn generate() -> Self { Self(Uuid::new_v4()) }
    pub fn into_inner(self) -> Uuid { self.0 }
}

impl std::fmt::Display for TaxTemplateId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::str::FromStr for TaxTemplateId {
    type Err = uuid::Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self(Uuid::parse_str(s)?))
    }
}

impl From<Uuid> for TaxTemplateId {
    fn from(id: Uuid) -> Self { Self(id) }
}

impl From<TaxTemplateId> for Uuid {
    fn from(id: TaxTemplateId) -> Self { id.0 }
}

impl AsRef<Uuid> for TaxTemplateId {
    fn as_ref(&self) -> &Uuid { &self.0 }
}

impl std::ops::Deref for TaxTemplateId {
    type Target = Uuid;
    fn deref(&self) -> &Self::Target { &self.0 }
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct TaxTemplate {
    pub id: Uuid,
    pub company_id: Uuid,
    pub code: String,
    pub name: String,
    pub template_type: TemplateType,
    pub tax_category_id: Option<Uuid>,
    pub is_inclusive: bool,
    pub status: TaxStatus,
    #[serde(default)]
    #[sqlx(json)]
    pub metadata: AuditMetadata,
}

impl TaxTemplate {
    /// Create a builder for TaxTemplate
    pub fn builder() -> TaxTemplateBuilder {
        TaxTemplateBuilder::default()
    }

    /// Create a new TaxTemplate with required fields
    pub fn new(company_id: Uuid, code: String, name: String, template_type: TemplateType, is_inclusive: bool, status: TaxStatus) -> Self {
        Self {
            id: Uuid::new_v4(),
            company_id,
            code,
            name,
            template_type,
            tax_category_id: None,
            is_inclusive,
            status,
            metadata: AuditMetadata::default(),
        }
    }

    /// Get the entity's unique identifier
    pub fn id(&self) -> &Uuid {
        &self.id
    }

    /// Get a strongly-typed ID for this entity
    pub fn typed_id(&self) -> TaxTemplateId {
        TaxTemplateId(self.id)
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

    /// Set the tax_category_id field (chainable)
    pub fn with_tax_category_id(mut self, value: Uuid) -> Self {
        self.tax_category_id = Some(value);
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
                "code" => {
                    if let Ok(v) = serde_json::from_value(value) { self.code = v; }
                }
                "name" => {
                    if let Ok(v) = serde_json::from_value(value) { self.name = v; }
                }
                "template_type" => {
                    if let Ok(v) = serde_json::from_value(value) { self.template_type = v; }
                }
                "tax_category_id" => {
                    if let Ok(v) = serde_json::from_value(value) { self.tax_category_id = v; }
                }
                "is_inclusive" => {
                    if let Ok(v) = serde_json::from_value(value) { self.is_inclusive = v; }
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

impl super::Entity for TaxTemplate {
    type Id = Uuid;

    fn entity_id(&self) -> &Self::Id {
        &self.id
    }

    fn entity_type() -> &'static str {
        "TaxTemplate"
    }
}

impl backbone_core::PersistentEntity for TaxTemplate {
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

impl backbone_orm::EntityRepoMeta for TaxTemplate {
    fn column_types() -> std::collections::HashMap<String, String> {
        let mut m = std::collections::HashMap::new();
        m.insert("id".to_string(), "uuid".to_string());
        m.insert("company_id".to_string(), "uuid".to_string());
        m.insert("tax_category_id".to_string(), "uuid".to_string());
        m.insert("template_type".to_string(), "template_type".to_string());
        m.insert("status".to_string(), "tax_status".to_string());
        m
    }
    fn search_fields() -> &'static [&'static str] {
        &["code", "name"]
    }
    fn company_field() -> Option<&'static str> {
        Some("company_id")
    }
}

/// Builder for TaxTemplate entity
///
/// Provides a fluent API for constructing TaxTemplate instances.
/// System fields (id, metadata, timestamps) are auto-initialized.
#[derive(Debug, Clone, Default)]
pub struct TaxTemplateBuilder {
    company_id: Option<Uuid>,
    code: Option<String>,
    name: Option<String>,
    template_type: Option<TemplateType>,
    tax_category_id: Option<Uuid>,
    is_inclusive: Option<bool>,
    status: Option<TaxStatus>,
}

impl TaxTemplateBuilder {
    /// Set the company_id field (required)
    pub fn company_id(mut self, value: Uuid) -> Self {
        self.company_id = Some(value);
        self
    }

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

    /// Set the template_type field (default: `TemplateType::default()`)
    pub fn template_type(mut self, value: TemplateType) -> Self {
        self.template_type = Some(value);
        self
    }

    /// Set the tax_category_id field (optional)
    pub fn tax_category_id(mut self, value: Uuid) -> Self {
        self.tax_category_id = Some(value);
        self
    }

    /// Set the is_inclusive field (default: `false`)
    pub fn is_inclusive(mut self, value: bool) -> Self {
        self.is_inclusive = Some(value);
        self
    }

    /// Set the status field (default: `TaxStatus::default()`)
    pub fn status(mut self, value: TaxStatus) -> Self {
        self.status = Some(value);
        self
    }

    /// Build the TaxTemplate entity
    ///
    /// Returns Err if any required field without a default is missing.
    pub fn build(self) -> Result<TaxTemplate, String> {
        let company_id = self.company_id.ok_or_else(|| "company_id is required".to_string())?;
        let code = self.code.ok_or_else(|| "code is required".to_string())?;
        let name = self.name.ok_or_else(|| "name is required".to_string())?;

        Ok(TaxTemplate {
            id: Uuid::new_v4(),
            company_id,
            code,
            name,
            template_type: self.template_type.unwrap_or(TemplateType::default()),
            tax_category_id: self.tax_category_id,
            is_inclusive: self.is_inclusive.unwrap_or(false),
            status: self.status.unwrap_or(TaxStatus::default()),
            metadata: AuditMetadata::default(),
        })
    }
}
