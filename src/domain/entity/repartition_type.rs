use serde::{Deserialize, Serialize};
use sqlx::Type;
use std::str::FromStr;
#[cfg(feature = "openapi")]
use utoipa::ToSchema;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
#[serde(rename_all = "snake_case")]
#[sqlx(type_name = "repartition_type", rename_all = "snake_case")]
pub enum RepartitionType {
    Base,
    Tax,
}

impl std::fmt::Display for RepartitionType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Base => write!(f, "base"),
            Self::Tax => write!(f, "tax"),
        }
    }
}

impl FromStr for RepartitionType {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "base" => Ok(Self::Base),
            "tax" => Ok(Self::Tax),
            _ => Err(format!("Unknown RepartitionType variant: {}", s)),
        }
    }
}

impl Default for RepartitionType {
    fn default() -> Self {
        Self::Tax
    }
}
