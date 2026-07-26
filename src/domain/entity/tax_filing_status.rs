use serde::{Deserialize, Serialize};
use sqlx::Type;
use std::str::FromStr;
#[cfg(feature = "openapi")]
use utoipa::ToSchema;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
#[serde(rename_all = "snake_case")]
#[sqlx(type_name = "tax_filing_status", rename_all = "snake_case")]
pub enum TaxFilingStatus {
    Open,
    Finalized,
    Filed,
}

impl std::fmt::Display for TaxFilingStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Open => write!(f, "open"),
            Self::Finalized => write!(f, "finalized"),
            Self::Filed => write!(f, "filed"),
        }
    }
}

impl FromStr for TaxFilingStatus {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "open" => Ok(Self::Open),
            "finalized" => Ok(Self::Finalized),
            "filed" => Ok(Self::Filed),
            _ => Err(format!("Unknown TaxFilingStatus variant: {}", s)),
        }
    }
}

impl Default for TaxFilingStatus {
    fn default() -> Self {
        Self::Open
    }
}
