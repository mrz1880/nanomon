use serde::{Deserialize, Serialize};

/// A systemd service unit
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemdService {
    pub name: String,
    pub description: String,
    pub state: ServiceState,
    pub sub_state: String,
}

impl SystemdService {
    pub fn new(name: String, description: String, state: ServiceState, sub_state: String) -> Self {
        Self {
            name,
            description,
            state,
            sub_state,
        }
    }
}

/// State of a systemd service
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum ServiceState {
    Active,
    Inactive,
    Failed,
    Activating,
    Deactivating,
    Unknown,
}

impl From<&str> for ServiceState {
    fn from(s: &str) -> Self {
        match s {
            "active" => Self::Active,
            "inactive" => Self::Inactive,
            "failed" => Self::Failed,
            "activating" => Self::Activating,
            "deactivating" => Self::Deactivating,
            _ => Self::Unknown,
        }
    }
}
