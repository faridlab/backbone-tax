use serde::{Deserialize, Serialize};
use sqlx::Type;
use std::str::FromStr;
#[cfg(feature = "openapi")]
use utoipa::ToSchema;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
#[serde(rename_all = "snake_case")]
#[sqlx(type_name = "e_faktur_status", rename_all = "snake_case")]
pub enum EFakturStatus {
    Assigned,
    Confirmed,
    Voided,
}

impl std::fmt::Display for EFakturStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Assigned => write!(f, "assigned"),
            Self::Confirmed => write!(f, "confirmed"),
            Self::Voided => write!(f, "voided"),
        }
    }
}

impl FromStr for EFakturStatus {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "assigned" => Ok(Self::Assigned),
            "confirmed" => Ok(Self::Confirmed),
            "voided" => Ok(Self::Voided),
            _ => Err(format!("Unknown EFakturStatus variant: {}", s)),
        }
    }
}

impl Default for EFakturStatus {
    fn default() -> Self {
        Self::Assigned
    }
}
