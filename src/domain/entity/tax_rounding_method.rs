use serde::{Deserialize, Serialize};
use sqlx::Type;
use std::str::FromStr;
#[cfg(feature = "openapi")]
use utoipa::ToSchema;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
#[serde(rename_all = "snake_case")]
#[sqlx(type_name = "tax_rounding_method", rename_all = "snake_case")]
pub enum TaxRoundingMethod {
    RoundGlobally,
    RoundPerLine,
}

impl std::fmt::Display for TaxRoundingMethod {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::RoundGlobally => write!(f, "round_globally"),
            Self::RoundPerLine => write!(f, "round_per_line"),
        }
    }
}

impl FromStr for TaxRoundingMethod {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "round_globally" => Ok(Self::RoundGlobally),
            "round_per_line" => Ok(Self::RoundPerLine),
            _ => Err(format!("Unknown TaxRoundingMethod variant: {}", s)),
        }
    }
}

impl Default for TaxRoundingMethod {
    fn default() -> Self {
        Self::RoundGlobally
    }
}
