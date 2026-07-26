use serde::{Deserialize, Serialize};
use sqlx::Type;
use std::str::FromStr;
#[cfg(feature = "openapi")]
use utoipa::ToSchema;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
#[serde(rename_all = "snake_case")]
#[sqlx(type_name = "tax_transaction_status", rename_all = "snake_case")]
pub enum TaxTransactionStatus {
    Recorded,
    Confirmed,
    Voided,
}

impl std::fmt::Display for TaxTransactionStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Recorded => write!(f, "recorded"),
            Self::Confirmed => write!(f, "confirmed"),
            Self::Voided => write!(f, "voided"),
        }
    }
}

impl FromStr for TaxTransactionStatus {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "recorded" => Ok(Self::Recorded),
            "confirmed" => Ok(Self::Confirmed),
            "voided" => Ok(Self::Voided),
            _ => Err(format!("Unknown TaxTransactionStatus variant: {}", s)),
        }
    }
}

impl Default for TaxTransactionStatus {
    fn default() -> Self {
        Self::Recorded
    }
}
