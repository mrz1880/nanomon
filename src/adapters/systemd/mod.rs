use async_trait::async_trait;
use serde::Deserialize;

use crate::domain::{ServiceState, SystemdService};
use crate::ports::ServiceSource;

/// Adapter that shells out to `systemctl` for service status.
/// No D-Bus dependency — keeps binary small.
pub struct SystemctlAdapter {
    available: bool,
    /// Optional glob allow-list; empty means "all units".
    filter: Vec<String>,
}

impl SystemctlAdapter {
    pub fn new(filter: Vec<String>) -> Self {
        let available = std::process::Command::new("systemctl")
            .arg("--version")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false);

        if available {
            tracing::info!("Systemd detected, service monitoring enabled");
        } else {
            tracing::debug!("Systemd not available, service monitoring disabled");
        }

        Self { available, filter }
    }
}

/// Match a unit name against a single glob pattern supporting `*` wildcards.
fn glob_match(pattern: &str, text: &str) -> bool {
    let parts: Vec<&str> = pattern.split('*').collect();
    if parts.len() == 1 {
        return pattern == text; // no wildcard -> exact match
    }

    let mut pos = 0;
    let last = parts.len() - 1;
    for (i, part) in parts.iter().enumerate() {
        if part.is_empty() {
            continue;
        }
        if i == 0 {
            if !text[pos..].starts_with(part) {
                return false;
            }
            pos += part.len();
        } else if i == last {
            if !text[pos..].ends_with(part) {
                return false;
            }
        } else {
            match text[pos..].find(part) {
                Some(idx) => pos += idx + part.len(),
                None => return false,
            }
        }
    }
    true
}

/// Whether a unit passes the filter. An empty filter allows everything.
fn unit_matches(name: &str, patterns: &[String]) -> bool {
    patterns.is_empty() || patterns.iter().any(|p| glob_match(p, name))
}

#[derive(Debug, Deserialize)]
struct SystemctlUnit {
    unit: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    active: Option<String>,
    #[serde(default)]
    sub: Option<String>,
}

#[async_trait]
impl ServiceSource for SystemctlAdapter {
    async fn list_services(
        &self,
    ) -> Result<Vec<SystemdService>, Box<dyn std::error::Error + Send + Sync>> {
        if !self.available {
            return Ok(Vec::new());
        }

        let output = tokio::process::Command::new("systemctl")
            .args([
                "list-units",
                "--type=service",
                "--all",
                "--output=json",
                "--no-pager",
            ])
            .output()
            .await?;

        if !output.status.success() {
            tracing::warn!(
                "systemctl failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            return Ok(Vec::new());
        }

        let units: Vec<SystemctlUnit> = serde_json::from_slice(&output.stdout).unwrap_or_default();

        let services = units
            .into_iter()
            .filter_map(|u| {
                let name = u.unit?;
                // Strip .service suffix for cleaner display
                let display_name = name.strip_suffix(".service").unwrap_or(&name).to_string();
                Some(SystemdService::new(
                    display_name,
                    u.description.unwrap_or_default(),
                    ServiceState::from(u.active.as_deref().unwrap_or("unknown")),
                    u.sub.unwrap_or_default(),
                ))
            })
            .filter(|s| unit_matches(&s.name, &self.filter))
            .collect();

        Ok(services)
    }

    fn is_available(&self) -> bool {
        self.available
    }
}

#[cfg(test)]
mod tests {
    use super::{glob_match, unit_matches};

    #[test]
    fn exact_and_wildcard_matches() {
        assert!(glob_match("nginx", "nginx"));
        assert!(!glob_match("nginx", "nginxx"));

        assert!(glob_match("worker-*", "worker-1"));
        assert!(glob_match("worker-*", "worker-abc"));
        assert!(glob_match("worker-*", "worker-")); // star matches empty
        assert!(!glob_match("worker-*", "worker")); // missing the dash

        assert!(glob_match("*-service", "docker-service"));
        assert!(glob_match("ng*x", "nginx"));
        assert!(!glob_match("ng*x", "apache"));
    }

    #[test]
    fn empty_filter_allows_all() {
        assert!(unit_matches("anything", &[]));
    }

    #[test]
    fn filter_restricts_to_matching_units() {
        let patterns = vec!["nginx".to_string(), "worker-*".to_string()];
        assert!(unit_matches("nginx", &patterns));
        assert!(unit_matches("worker-7", &patterns));
        assert!(!unit_matches("sshd", &patterns));
    }
}
