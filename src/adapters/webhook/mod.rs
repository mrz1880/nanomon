use std::time::Duration;

use async_trait::async_trait;

use crate::domain::{AlertEvent, AlertKind, ServiceAlertEvent, Severity};
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

/// A notification ready to be posted to ntfy.
struct Notification {
    title: String,
    body: String,
    /// ntfy tag, rendered as an emoji in the client.
    tags: &'static str,
    priority: &'static str,
}

/// Build the ntfy notification for a numeric alert.
///
/// A resolution is deliberately quiet: it closes an incident the operator has
/// already been told about, so it must not buzz like a new one.
fn notification_for(event: &AlertEvent) -> Notification {
    let (tags, priority) = match (event.kind, event.severity) {
        (AlertKind::Resolved, _) => ("white_check_mark", "low"),
        (AlertKind::Firing, Severity::Critical) => ("rotating_light", "urgent"),
        (AlertKind::Firing, Severity::Warning) => ("warning", "high"),
        (AlertKind::Firing, Severity::Info) => ("information_source", "default"),
    };

    let title = match event.kind {
        AlertKind::Firing => event.rule_name.clone(),
        AlertKind::Resolved => format!("Resolved: {}", event.rule_name),
    };

    Notification {
        title,
        body: format_alert(event),
        tags,
        priority,
    }
}

/// Render a duration the way an operator would say it out loud.
fn format_duration(seconds: u64) -> String {
    match seconds {
        s if s < 60 => format!("{s}s"),
        s if s < 3600 => format!("{}m", s / 60),
        s => format!("{}h {}m", s / 3600, (s % 3600) / 60),
    }
}

/// Notification body for a numeric alert.
///
/// Reads as a sentence: what the value is, what it was compared against, and
/// the context needed to judge it without opening the dashboard.
fn format_alert(event: &AlertEvent) -> String {
    let unit = &event.unit;
    let mut body = match event.kind {
        AlertKind::Firing => format!(
            "{} is {:.1}{unit} ({} {:.1}{unit})",
            event.metric,
            event.current_value,
            event.condition.to_lowercase(),
            event.threshold,
        ),
        AlertKind::Resolved => format!(
            "{} is back to {:.1}{unit} (threshold {:.1}{unit})",
            event.metric, event.current_value, event.threshold,
        ),
    };

    if let Some(warn_at) = event.warn_at {
        body.push_str(&format!("\nhardware warns at {warn_at:.1}{unit}"));
    }
    if let Some(seconds) = event.breached_for_seconds {
        body.push_str(&format!("\nlasted {}", format_duration(seconds)));
    }
    body.push_str(&format!("\nhost {}", event.hostname));
    body
}

/// (title, body) notification text for a systemd service alert.
fn format_service_alert(event: &ServiceAlertEvent) -> (String, String) {
    let body = format!(
        "unit {} is {}\nhost: {}",
        event.unit, event.state, event.hostname
    );
    (event.rule_name.clone(), body)
}

/// A webhook that answered, but rejected the notification.
#[derive(Debug, thiserror::Error)]
#[error("webhook rejected '{rule_name}' with status {status}")]
struct WebhookRejected {
    status: reqwest::StatusCode,
    rule_name: String,
}

/// Whether the POST actually delivered the notification.
///
/// A non-2xx answer means nobody was notified, most often a wrong topic in the
/// URL. Reporting it as a success would let the evaluator mark the incident
/// announced and go quiet for its whole duration, so it has to surface as an
/// error: the caller then leaves the incident un-notified and retries.
///
/// # Arguments
/// * `status` - HTTP status the webhook answered with.
/// * `rule_name` - Rule being announced, for the error message.
///
/// # Errors
/// Returns [`WebhookRejected`] when `status` is not a success status.
fn delivery_outcome(status: reqwest::StatusCode, rule_name: &str) -> Result<(), WebhookRejected> {
    if status.is_success() {
        return Ok(());
    }
    Err(WebhookRejected {
        status,
        rule_name: rule_name.to_string(),
    })
}

#[async_trait]
impl AlertSink for WebhookSink {
    async fn send_alert(
        &self,
        url: &str,
        event: &AlertEvent,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let response = if is_ntfy(url) {
            let notification = notification_for(event);
            self.client
                .post(url)
                .header("Title", notification.title)
                .header("Tags", notification.tags)
                .header("Priority", notification.priority)
                .body(notification.body)
                .send()
                .await?
        } else {
            self.client.post(url).json(event).send().await?
        };

        delivery_outcome(response.status(), &event.rule_name)?;
        tracing::info!(
            "Alert '{}' ({}) sent to webhook",
            event.rule_name,
            event.kind
        );

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

        delivery_outcome(response.status(), &event.rule_name)?;
        tracing::info!("Service alert '{}' sent to webhook", event.rule_name);

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{
        delivery_outcome, format_alert, format_duration, format_service_alert, is_ntfy,
        notification_for,
    };
    use crate::domain::{AlertEvent, AlertKind, ServiceAlertEvent, Severity};
    use reqwest::StatusCode;

    #[test]
    fn detects_ntfy_urls() {
        assert!(is_ntfy("https://ntfy.sh/mytopic"));
        assert!(is_ntfy("http://ntfy.example.com/t"));
        assert!(!is_ntfy("https://hooks.slack.com/services/x"));
    }

    fn event(kind: AlertKind, severity: Severity) -> AlertEvent {
        AlertEvent {
            kind,
            severity,
            rule_name: "Disk almost full".to_string(),
            metric: "Disk usage /volume1".to_string(),
            unit: "%".to_string(),
            current_value: 86.3456,
            threshold: 85.0,
            condition: "Above".to_string(),
            hostname: "nas".to_string(),
            timestamp: "2026-09-04T00:00:00Z".to_string(),
            breached_for_seconds: None,
            warn_at: None,
        }
    }

    #[test]
    fn severity_drives_how_loudly_the_breach_arrives() {
        let critical = notification_for(&event(AlertKind::Firing, Severity::Critical));
        assert_eq!(critical.priority, "urgent");
        assert_eq!(critical.tags, "rotating_light");

        let warning = notification_for(&event(AlertKind::Firing, Severity::Warning));
        assert_eq!(warning.priority, "high");
        assert_eq!(warning.tags, "warning");

        let info = notification_for(&event(AlertKind::Firing, Severity::Info));
        assert_eq!(info.priority, "default");
        assert_eq!(info.tags, "information_source");
    }

    #[test]
    fn a_resolution_arrives_quietly_whatever_the_severity() {
        let n = notification_for(&event(AlertKind::Resolved, Severity::Critical));

        assert_eq!(n.priority, "low");
        assert_eq!(n.tags, "white_check_mark");
        // The title must say it is over without hunting through the body.
        assert_eq!(n.title, "Resolved: Disk almost full");
    }

    #[test]
    fn a_breach_keeps_the_rule_name_as_title() {
        let n = notification_for(&event(AlertKind::Firing, Severity::Warning));
        assert_eq!(n.title, "Disk almost full");
    }

    #[test]
    fn the_body_states_the_value_with_its_unit_and_the_threshold() {
        let body = format_alert(&event(AlertKind::Firing, Severity::Warning));

        assert!(body.contains("Disk usage /volume1 is 86.3%"), "{body}");
        assert!(body.contains("above 85.0%"), "{body}");
        assert!(body.contains("host nas"), "{body}");
    }

    #[test]
    fn the_body_mentions_the_hardware_limit_when_the_sensor_publishes_one() {
        let mut e = event(AlertKind::Firing, Severity::Warning);
        e.metric = "Temperature Package id 0".to_string();
        e.unit = "C".to_string();
        e.current_value = 92.0;
        e.threshold = 90.0;
        e.warn_at = Some(100.0);

        let body = format_alert(&e);

        // Without this line 92 C looks alarming; with it, it plainly is not.
        assert!(body.contains("hardware warns at 100.0C"), "{body}");
    }

    #[test]
    fn a_resolution_says_how_long_the_incident_lasted() {
        let mut e = event(AlertKind::Resolved, Severity::Warning);
        e.current_value = 40.0;
        e.breached_for_seconds = Some(11_520); // 3h 12m

        let body = format_alert(&e);

        assert!(body.contains("back to 40.0%"), "{body}");
        assert!(body.contains("lasted 3h 12m"), "{body}");
    }

    #[test]
    fn durations_read_naturally_at_every_scale() {
        assert_eq!(format_duration(45), "45s");
        assert_eq!(format_duration(600), "10m");
        assert_eq!(format_duration(3600), "1h 0m");
        assert_eq!(format_duration(11_520), "3h 12m");
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

    #[test]
    fn a_rejected_post_is_a_delivery_failure() {
        // A wrong topic or a server error means nobody was notified. Reporting
        // it as success would let the evaluator close the incident and go
        // quiet, so the status has to surface as an error and be retried.
        let rejected = delivery_outcome(StatusCode::NOT_FOUND, "High load average");
        let message = rejected
            .expect_err("a 404 must not count as delivered")
            .to_string();
        assert!(message.contains("404"), "{message}");
        assert!(message.contains("High load average"), "{message}");

        assert!(delivery_outcome(StatusCode::OK, "High load average").is_ok());
    }
}
