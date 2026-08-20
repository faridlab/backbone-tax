use serde::{Deserialize, Serialize};
use sqlx::Type;
use std::str::FromStr;
#[cfg(feature = "openapi")]
use utoipa::ToSchema;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
#[serde(rename_all = "snake_case")]
#[sqlx(type_name = "repartition_document_type", rename_all = "snake_case")]
pub enum RepartitionDocumentType {
    Invoice,
    Refund,
}

impl std::fmt::Display for RepartitionDocumentType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Invoice => write!(f, "invoice"),
            Self::Refund => write!(f, "refund"),
        }
    }
}

impl FromStr for RepartitionDocumentType {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "invoice" => Ok(Self::Invoice),
            "refund" => Ok(Self::Refund),
            _ => Err(format!("Unknown RepartitionDocumentType variant: {}", s)),
        }
    }
}

impl Default for RepartitionDocumentType {
    fn default() -> Self {
        Self::Invoice
    }
}
