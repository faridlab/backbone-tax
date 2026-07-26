use serde::{Deserialize, Serialize};
use sqlx::Type;
use std::str::FromStr;
#[cfg(feature = "openapi")]
use utoipa::ToSchema;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
#[serde(rename_all = "snake_case")]
#[sqlx(type_name = "tax_transaction_source", rename_all = "snake_case")]
pub enum TaxTransactionSource {
    Seam,
    ComputedLive,
}

impl std::fmt::Display for TaxTransactionSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Seam => write!(f, "seam"),
            Self::ComputedLive => write!(f, "computed_live"),
        }
    }
}

impl FromStr for TaxTransactionSource {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "seam" => Ok(Self::Seam),
            "computed_live" => Ok(Self::ComputedLive),
            _ => Err(format!("Unknown TaxTransactionSource variant: {}", s)),
        }
    }
}

impl Default for TaxTransactionSource {
    fn default() -> Self {
        Self::Seam
    }
}
