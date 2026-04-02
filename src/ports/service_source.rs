use async_trait::async_trait;

use crate::domain::SystemdService;

/// Port for fetching systemd service status
#[async_trait]
pub trait ServiceSource: Send + Sync {
    /// List all services
    async fn list_services(
        &self,
    ) -> Result<Vec<SystemdService>, Box<dyn std::error::Error + Send + Sync>>;

    /// Check if the service source is available on this system
    fn is_available(&self) -> bool;
}
