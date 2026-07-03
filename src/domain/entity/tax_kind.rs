use serde::{Deserialize, Serialize};
use sqlx::Type;
use std::str::FromStr;
#[cfg(feature = "openapi")]
use utoipa::ToSchema;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
#[serde(rename_all = "snake_case")]
#[sqlx(type_name = "tax_kind", rename_all = "snake_case")]
pub enum TaxKind {
    Vat,
    Withholding,
    Sales,
    Other,
}

impl std::fmt::Display for TaxKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Vat => write!(f, "vat"),
            Self::Withholding => write!(f, "withholding"),
            Self::Sales => write!(f, "sales"),
            Self::Other => write!(f, "other"),
        }
    }
}

impl FromStr for TaxKind {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "vat" => Ok(Self::Vat),
            "withholding" => Ok(Self::Withholding),
            "sales" => Ok(Self::Sales),
            "other" => Ok(Self::Other),
            _ => Err(format!("Unknown TaxKind variant: {}", s)),
        }
    }
}

impl Default for TaxKind {
    fn default() -> Self {
        Self::Vat
    }
}
