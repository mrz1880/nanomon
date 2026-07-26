use std::time::Duration;

use async_trait::async_trait;

use crate::domain::{AlertEvent, ServiceAlertEvent};
use crate::ports::AlertSink;

/// Sends alert events to a webhook URL.
///
/// For ntfy targets the payload is a human-readable text notification (title +
/// body + tag); for any other URL it stays a JSON POST of the event, so generic
/// receivers (Slack, Discord, custom) keep the structured contract.
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

/// Whether a webhook URL points at an ntfy server (needs text, not JSON).
fn is_ntfy(url: &str) -> bool {
    url.contains("ntfy")
}

/// (title, body) notification text for a numeric alert.
fn format_alert(event: &AlertEvent) -> (String, String) {
    let body = format!(
        "{} = {:.1} (threshold: {} {:.1})\nhost: {}",
        event.metric, event.current_value, event.condition, event.threshold, event.hostname
    );
    (event.rule_name.clone(), body)
}

/// (title, body) notification text for a systemd service alert.
fn format_service_alert(event: &ServiceAlertEvent) -> (String, String) {
    let body = format!(
        "unit {} is {}\nhost: {}",
        event.unit, event.state, event.hostname
    );
    (event.rule_name.clone(), body)
}

#[async_trait]
impl AlertSink for WebhookSink {
    async fn send_alert(
        &self,
        url: &str,
        event: &AlertEvent,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let response = if is_ntfy(url) {
            let (title, body) = format_alert(event);
            self.client
                .post(url)
                .header("Title", title)
                .header("Tags", "warning")
                .header("Priority", "high")
                .body(body)
                .send()
                .await?
        } else {
            self.client.post(url).json(event).send().await?
        };

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

    async fn send_service_alert(
        &self,
        url: &str,
        event: &ServiceAlertEvent,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let response = if is_ntfy(url) {
            let (title, body) = format_service_alert(event);
            self.client
                .post(url)
                .header("Title", title)
                .header("Tags", "rotating_light")
                .header("Priority", "urgent")
                .body(body)
                .send()
                .await?
        } else {
            self.client.post(url).json(event).send().await?
        };

        if !response.status().is_success() {
            tracing::warn!(
                "Webhook returned status {} for service alert '{}'",
                response.status(),
                event.rule_name
            );
        } else {
            tracing::info!("Service alert '{}' sent to webhook", event.rule_name);
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{format_alert, format_service_alert, is_ntfy};
    use crate::domain::{AlertEvent, ServiceAlertEvent};

    #[test]
    fn detects_ntfy_urls() {
        assert!(is_ntfy("https://ntfy.sh/mytopic"));
        assert!(is_ntfy("http://ntfy.example.com/t"));
        assert!(!is_ntfy("https://hooks.slack.com/services/x"));
    }

    #[test]
    fn formats_numeric_alert() {
        let event = AlertEvent {
            rule_name: "Disk almost full".to_string(),
            metric: "DiskUsage { mount_point: \"/\" }".to_string(),
            current_value: 86.3456,
            threshold: 85.0,
            condition: "Above".to_string(),
            hostname: "nas".to_string(),
            timestamp: "2026-07-26T00:00:00Z".to_string(),
        };
        let (title, body) = format_alert(&event);
        assert_eq!(title, "Disk almost full");
        assert!(body.contains("= 86.3"));
        assert!(body.contains("threshold: Above 85.0"));
        assert!(body.contains("host: nas"));
    }

    #[test]
    fn formats_service_alert() {
        let event = ServiceAlertEvent {
            rule_name: "nginx down".to_string(),
            unit: "nginx".to_string(),
            state: "failed".to_string(),
            hostname: "nas".to_string(),
            timestamp: "2026-07-26T00:00:00Z".to_string(),
        };
        let (title, body) = format_service_alert(&event);
        assert_eq!(title, "nginx down");
        assert!(body.contains("unit nginx is failed"));
        assert!(body.contains("host: nas"));
    }
}
