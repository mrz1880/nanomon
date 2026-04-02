use async_trait::async_trait;

use crate::domain::AlertEvent;

/// Port for sending alert notifications
#[async_trait]
pub trait AlertSink: Send + Sync {
    /// Send an alert event to the given webhook URL
    async fn send_alert(
        &self,
        url: &str,
        event: &AlertEvent,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>>;
}
