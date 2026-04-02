use std::time::Duration;

use async_trait::async_trait;

use crate::domain::AlertEvent;
use crate::ports::AlertSink;

/// Sends alert events as JSON via HTTP POST to a webhook URL
pub struct WebhookSink {
    client: reqwest::Client,
}

impl WebhookSink {
    pub fn new() -> Self {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(5))
            .build()
            .expect("Failed to build HTTP client");

        Self { client }
    }
}

#[async_trait]
impl AlertSink for WebhookSink {
    async fn send_alert(
        &self,
        url: &str,
        event: &AlertEvent,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let response = self.client.post(url).json(event).send().await?;

        if !response.status().is_success() {
            tracing::warn!(
                "Webhook returned status {} for alert '{}'",
                response.status(),
                event.rule_name
            );
        } else {
            tracing::info!("Alert '{}' sent to webhook", event.rule_name);
        }

        Ok(())
    }
}
