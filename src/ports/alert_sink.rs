use async_trait::async_trait;

use crate::domain::{AlertEvent, ServiceAlertEvent};

/// Port for sending alert notifications
#[async_trait]
pub trait AlertSink: Send + Sync {
    /// Send a numeric alert event to the given webhook URL
    async fn send_alert(
        &self,
        url: &str,
        event: &AlertEvent,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>>;

    /// Send a systemd service alert event to the given webhook URL
    async fn send_service_alert(
        &self,
        url: &str,
        event: &ServiceAlertEvent,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>>;
}
