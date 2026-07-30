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

#[cfg(test)]
mod tests {
    use super::{ServiceState, SystemdService};

    #[test]
    fn parses_every_systemctl_state() {
        assert_eq!(ServiceState::from("active"), ServiceState::Active);
        assert_eq!(ServiceState::from("inactive"), ServiceState::Inactive);
        assert_eq!(ServiceState::from("failed"), ServiceState::Failed);
        assert_eq!(ServiceState::from("activating"), ServiceState::Activating);
        assert_eq!(
            ServiceState::from("deactivating"),
            ServiceState::Deactivating
        );
    }

    #[test]
    fn unrecognized_states_fall_back_to_unknown() {
        assert_eq!(ServiceState::from("reloading"), ServiceState::Unknown);
        assert_eq!(ServiceState::from(""), ServiceState::Unknown);
        // Parsing is case-sensitive: systemctl always reports lowercase.
        assert_eq!(ServiceState::from("Active"), ServiceState::Unknown);
    }

    #[test]
    fn new_keeps_every_field() {
        let s = SystemdService::new(
            "nginx".to_string(),
            "A web server".to_string(),
            ServiceState::Active,
            "running".to_string(),
        );
        assert_eq!(s.name, "nginx");
        assert_eq!(s.description, "A web server");
        assert_eq!(s.state, ServiceState::Active);
        assert_eq!(s.sub_state, "running");
    }

    #[test]
    fn state_serializes_lowercase() {
        let json = serde_json::to_string(&ServiceState::Failed).unwrap();
        assert_eq!(json, "\"failed\"");
    }
}
