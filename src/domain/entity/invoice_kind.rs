use serde::{Deserialize, Serialize};
use sqlx::Type;
use std::str::FromStr;
#[cfg(feature = "openapi")]
use utoipa::ToSchema;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
#[serde(rename_all = "snake_case")]
#[sqlx(type_name = "invoice_kind", rename_all = "snake_case")]
pub enum InvoiceKind {
    Sales,
    Purchase,
}

impl std::fmt::Display for InvoiceKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Sales => write!(f, "sales"),
            Self::Purchase => write!(f, "purchase"),
        }
    }
}

impl FromStr for InvoiceKind {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "sales" => Ok(Self::Sales),
            "purchase" => Ok(Self::Purchase),
            _ => Err(format!("Unknown InvoiceKind variant: {}", s)),
        }
    }
}

impl Default for InvoiceKind {
    fn default() -> Self {
        Self::Sales
    }
}
