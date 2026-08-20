use super::AuditMetadata;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

/// Strongly-typed ID for TaxTag
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct TaxTagId(pub Uuid);

impl TaxTagId {
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

impl std::fmt::Display for TaxTagId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::str::FromStr for TaxTagId {
    type Err = uuid::Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self(Uuid::parse_str(s)?))
    }
}

impl From<Uuid> for TaxTagId {
    fn from(id: Uuid) -> Self {
        Self(id)
    }
}

impl From<TaxTagId> for Uuid {
    fn from(id: TaxTagId) -> Self {
        id.0
    }
}

impl AsRef<Uuid> for TaxTagId {
    fn as_ref(&self) -> &Uuid {
        &self.0
    }
}

impl std::ops::Deref for TaxTagId {
    type Target = Uuid;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct TaxTag {
    pub id: Uuid,
    pub company_id: Uuid,
    pub code: String,
    pub name: String,
    #[serde(default)]
    #[sqlx(json)]
    pub metadata: AuditMetadata,
}

impl TaxTag {
    /// Create a builder for TaxTag
    pub fn builder() -> TaxTagBuilder {
        <TaxTagBuilder as Default>::default()
    }

    /// Create a new TaxTag with required fields
    pub fn new(company_id: Uuid, code: String, name: String) -> Self {
        Self {
            id: Uuid::new_v4(),
            company_id,
            code,
            name,
            metadata: AuditMetadata::default(),
        }
    }

    /// Get the entity's unique identifier
    pub fn id(&self) -> &Uuid {
        &self.id
    }

    /// Get a strongly-typed ID for this entity
    pub fn typed_id(&self) -> TaxTagId {
        TaxTagId(self.id)
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
                "code" => {
                    if let Ok(v) = serde_json::from_value(value) {
                        self.code = v;
                    }
                }
                "name" => {
                    if let Ok(v) = serde_json::from_value(value) {
                        self.name = v;
                    }
                }
                _ => {} // ignore unknown fields
            }
        }
    }

    // <<< CUSTOM METHODS START >>>
    // <<< CUSTOM METHODS END >>>
}

impl super::Entity for TaxTag {
    type Id = Uuid;

    fn entity_id(&self) -> &Self::Id {
        &self.id
    }

    fn entity_type() -> &'static str {
        "TaxTag"
    }
}

impl backbone_core::PersistentEntity for TaxTag {
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

impl backbone_orm::EntityRepoMeta for TaxTag {
    fn column_types() -> std::collections::HashMap<String, String> {
        let mut m = std::collections::HashMap::new();
        m.insert("id".to_string(), "uuid".to_string());
        m.insert("company_id".to_string(), "uuid".to_string());
        m
    }
    fn search_fields() -> &'static [&'static str] {
        &["code", "name"]
    }
    fn company_field() -> Option<&'static str> {
        Some("company_id")
    }
}

/// Builder for TaxTag entity
///
/// Provides a fluent API for constructing TaxTag instances.
/// System fields (id, metadata, timestamps) are auto-initialized.
#[derive(Debug, Clone, Default)]
pub struct TaxTagBuilder {
    company_id: Option<Uuid>,
    code: Option<String>,
    name: Option<String>,
}

impl TaxTagBuilder {
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

    /// Build the TaxTag entity
    ///
    /// Returns Err if any required field without a default is missing.
    pub fn build(self) -> Result<TaxTag, String> {
        let company_id = self
            .company_id
            .ok_or_else(|| "company_id is required".to_string())?;
        let code = self.code.ok_or_else(|| "code is required".to_string())?;
        let name = self.name.ok_or_else(|| "name is required".to_string())?;

        Ok(TaxTag {
            id: Uuid::new_v4(),
            company_id,
            code,
            name,
            metadata: AuditMetadata::default(),
        })
    }
}
