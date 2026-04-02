use async_trait::async_trait;
use serde::Deserialize;

use crate::domain::{ServiceState, SystemdService};
use crate::ports::ServiceSource;

/// Adapter that shells out to `systemctl` for service status.
/// No D-Bus dependency — keeps binary small.
pub struct SystemctlAdapter {
    available: bool,
}

impl SystemctlAdapter {
    pub fn new() -> Self {
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

        Self { available }
    }
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
            .collect();

        Ok(services)
    }

    fn is_available(&self) -> bool {
        self.available
    }
}
