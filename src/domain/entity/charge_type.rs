use serde::{Deserialize, Serialize};
use sqlx::Type;
use std::str::FromStr;
#[cfg(feature = "openapi")]
use utoipa::ToSchema;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
#[serde(rename_all = "snake_case")]
#[sqlx(type_name = "charge_type", rename_all = "snake_case")]
pub enum ChargeType {
    OnNetTotal,
    OnPreviousRowTotal,
    Actual,
}

impl std::fmt::Display for ChargeType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::OnNetTotal => write!(f, "on_net_total"),
            Self::OnPreviousRowTotal => write!(f, "on_previous_row_total"),
            Self::Actual => write!(f, "actual"),
        }
    }
}

impl FromStr for ChargeType {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "on_net_total" => Ok(Self::OnNetTotal),
            "on_previous_row_total" => Ok(Self::OnPreviousRowTotal),
            "actual" => Ok(Self::Actual),
            _ => Err(format!("Unknown ChargeType variant: {}", s)),
        }
    }
}

impl Default for ChargeType {
    fn default() -> Self {
        Self::OnNetTotal
    }
}
