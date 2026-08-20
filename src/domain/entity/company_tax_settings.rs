use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

use super::AuditMetadata;
use super::TaxExigibility;
use super::TaxRoundingMethod;

/// Strongly-typed ID for CompanyTaxSettings
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct CompanyTaxSettingsId(pub Uuid);

impl CompanyTaxSettingsId {
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

impl std::fmt::Display for CompanyTaxSettingsId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::str::FromStr for CompanyTaxSettingsId {
    type Err = uuid::Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self(Uuid::parse_str(s)?))
    }
}

impl From<Uuid> for CompanyTaxSettingsId {
    fn from(id: Uuid) -> Self {
        Self(id)
    }
}

impl From<CompanyTaxSettingsId> for Uuid {
    fn from(id: CompanyTaxSettingsId) -> Self {
        id.0
    }
}

impl AsRef<Uuid> for CompanyTaxSettingsId {
    fn as_ref(&self) -> &Uuid {
        &self.0
    }
}

impl std::ops::Deref for CompanyTaxSettingsId {
    type Target = Uuid;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct CompanyTaxSettings {
    pub id: Uuid,
    pub company_id: Uuid,
    pub rounding_method: TaxRoundingMethod,
    pub default_exigibility: TaxExigibility,
    pub cash_basis_transition_account_id: Option<Uuid>,
    #[serde(default)]
    #[sqlx(json)]
    pub metadata: AuditMetadata,
}

impl CompanyTaxSettings {
    /// Create a builder for CompanyTaxSettings
    pub fn builder() -> CompanyTaxSettingsBuilder {
        <CompanyTaxSettingsBuilder as Default>::default()
    }

    /// Create a new CompanyTaxSettings with required fields
    pub fn new(
        company_id: Uuid,
        rounding_method: TaxRoundingMethod,
        default_exigibility: TaxExigibility,
    ) -> Self {
        Self {
            id: Uuid::new_v4(),
            company_id,
            rounding_method,
            default_exigibility,
            cash_basis_transition_account_id: None,
            metadata: AuditMetadata::default(),
        }
    }

    /// Get the entity's unique identifier
    pub fn id(&self) -> &Uuid {
        &self.id
    }

    /// Get a strongly-typed ID for this entity
    pub fn typed_id(&self) -> CompanyTaxSettingsId {
        CompanyTaxSettingsId(self.id)
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

    /// Set the cash_basis_transition_account_id field (chainable)
    pub fn with_cash_basis_transition_account_id(mut self, value: Uuid) -> Self {
        self.cash_basis_transition_account_id = Some(value);
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
                "rounding_method" => {
                    if let Ok(v) = serde_json::from_value(value) {
                        self.rounding_method = v;
                    }
                }
                "default_exigibility" => {
                    if let Ok(v) = serde_json::from_value(value) {
                        self.default_exigibility = v;
                    }
                }
                "cash_basis_transition_account_id" => {
                    if let Ok(v) = serde_json::from_value(value) {
                        self.cash_basis_transition_account_id = v;
                    }
                }
                _ => {} // ignore unknown fields
            }
        }
    }

    // <<< CUSTOM METHODS START >>>
    // <<< CUSTOM METHODS END >>>
}

impl super::Entity for CompanyTaxSettings {
    type Id = Uuid;

    fn entity_id(&self) -> &Self::Id {
        &self.id
    }

    fn entity_type() -> &'static str {
        "CompanyTaxSettings"
    }
}

impl backbone_core::PersistentEntity for CompanyTaxSettings {
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

impl backbone_orm::EntityRepoMeta for CompanyTaxSettings {
    fn column_types() -> std::collections::HashMap<String, String> {
        let mut m = std::collections::HashMap::new();
        m.insert("id".to_string(), "uuid".to_string());
        m.insert("company_id".to_string(), "uuid".to_string());
        m.insert(
            "cash_basis_transition_account_id".to_string(),
            "uuid".to_string(),
        );
        m.insert(
            "rounding_method".to_string(),
            "tax_rounding_method".to_string(),
        );
        m.insert(
            "default_exigibility".to_string(),
            "tax_exigibility".to_string(),
        );
        m
    }
    fn search_fields() -> &'static [&'static str] {
        &[]
    }
    fn company_field() -> Option<&'static str> {
        Some("company_id")
    }
}

/// Builder for CompanyTaxSettings entity
///
/// Provides a fluent API for constructing CompanyTaxSettings instances.
/// System fields (id, metadata, timestamps) are auto-initialized.
#[derive(Debug, Clone, Default)]
pub struct CompanyTaxSettingsBuilder {
    company_id: Option<Uuid>,
    rounding_method: Option<TaxRoundingMethod>,
    default_exigibility: Option<TaxExigibility>,
    cash_basis_transition_account_id: Option<Uuid>,
}

impl CompanyTaxSettingsBuilder {
    /// Set the company_id field (required)
    pub fn company_id(mut self, value: Uuid) -> Self {
        self.company_id = Some(value);
        self
    }

    /// Set the rounding_method field (default: `TaxRoundingMethod::default()`)
    pub fn rounding_method(mut self, value: TaxRoundingMethod) -> Self {
        self.rounding_method = Some(value);
        self
    }

    /// Set the default_exigibility field (default: `TaxExigibility::default()`)
    pub fn default_exigibility(mut self, value: TaxExigibility) -> Self {
        self.default_exigibility = Some(value);
        self
    }

    /// Set the cash_basis_transition_account_id field (optional)
    pub fn cash_basis_transition_account_id(mut self, value: Uuid) -> Self {
        self.cash_basis_transition_account_id = Some(value);
        self
    }

    /// Build the CompanyTaxSettings entity
    ///
    /// Returns Err if any required field without a default is missing.
    pub fn build(self) -> Result<CompanyTaxSettings, String> {
        let company_id = self
            .company_id
            .ok_or_else(|| "company_id is required".to_string())?;

        Ok(CompanyTaxSettings {
            id: Uuid::new_v4(),
            company_id,
            rounding_method: self.rounding_method.unwrap_or_default(),
            default_exigibility: self.default_exigibility.unwrap_or_default(),
            cash_basis_transition_account_id: self.cash_basis_transition_account_id,
            metadata: AuditMetadata::default(),
        })
    }
}
