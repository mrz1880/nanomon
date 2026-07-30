use serde::{Deserialize, Serialize};

use super::ServiceState;

/// A rule defining when an alert should fire
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlertRule {
    pub name: String,
    pub metric: AlertMetric,
    pub condition: AlertCondition,
    pub threshold: f64,
    pub webhook_url: String,
    #[serde(default = "default_cooldown")]
    pub cooldown_seconds: u64,
}

fn default_cooldown() -> u64 {
    300 // 5 minutes
}

/// Which metric to evaluate
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AlertMetric {
    CpuUsage,
    MemoryUsage,
    DiskUsage { mount_point: String },
    LoadAverage1m,
    Temperature { label: String },
    External { name: String },
}

/// Comparison condition
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AlertCondition {
    Above,
    Below,
}

impl AlertCondition {
    pub fn evaluate(&self, current: f64, threshold: f64) -> bool {
        match self {
            Self::Above => current > threshold,
            Self::Below => current < threshold,
        }
    }
}

/// An alert event fired when a rule triggers
#[derive(Debug, Clone, Serialize)]
pub struct AlertEvent {
    pub rule_name: String,
    pub metric: String,
    pub current_value: f64,
    pub threshold: f64,
    pub condition: String,
    pub hostname: String,
    pub timestamp: String,
}

/// A rule that fires when a systemd unit enters a "bad" state.
///
/// Kept distinct from the numeric [`AlertRule`] because service health is
/// categorical (a set of states), not a threshold comparison.
#[derive(Debug, Clone, Deserialize)]
pub struct ServiceAlertRule {
    pub name: String,
    /// systemd unit name (without the `.service` suffix, e.g. `nginx`).
    pub unit: String,
    /// States considered "bad" — the alert fires when the unit enters one.
    pub alert_when: Vec<ServiceState>,
    pub webhook_url: String,
    #[serde(default = "default_cooldown")]
    pub cooldown_seconds: u64,
}

/// An alert event fired when a systemd unit enters a bad state.
#[derive(Debug, Clone, Serialize)]
pub struct ServiceAlertEvent {
    pub rule_name: String,
    pub unit: String,
    pub state: String,
    pub hostname: String,
    pub timestamp: String,
}

#[cfg(test)]
mod tests {
    use super::{default_cooldown, AlertCondition, AlertMetric, AlertRule, ServiceAlertRule};
    use crate::domain::ServiceState;

    #[test]
    fn above_is_strict() {
        let c = AlertCondition::Above;
        assert!(c.evaluate(90.1, 90.0));
        // Exactly at the threshold must not fire.
        assert!(!c.evaluate(90.0, 90.0));
        assert!(!c.evaluate(89.9, 90.0));
    }

    #[test]
    fn below_is_strict() {
        let c = AlertCondition::Below;
        assert!(c.evaluate(9.9, 10.0));
        assert!(!c.evaluate(10.0, 10.0));
        assert!(!c.evaluate(10.1, 10.0));
    }

    #[test]
    fn default_cooldown_is_five_minutes() {
        assert_eq!(default_cooldown(), 300);
    }

    #[test]
    fn rule_parses_from_toml_with_default_cooldown() {
        let rule: AlertRule = toml::from_str(
            r#"
            name = "High CPU"
            metric = "cpu_usage"
            condition = "above"
            threshold = 90.0
            webhook_url = "https://example/hook"
            "#,
        )
        .unwrap();

        assert_eq!(rule.name, "High CPU");
        assert!(matches!(rule.metric, AlertMetric::CpuUsage));
        assert!(matches!(rule.condition, AlertCondition::Above));
        assert_eq!(rule.threshold, 90.0);
        assert_eq!(rule.cooldown_seconds, default_cooldown());
    }

    #[test]
    fn nested_metrics_parse_from_toml() {
        let disk: AlertRule = toml::from_str(
            r#"
            name = "d"
            metric = { disk_usage = { mount_point = "/srv" } }
            condition = "above"
            threshold = 1.0
            webhook_url = "u"
            "#,
        )
        .unwrap();
        match disk.metric {
            AlertMetric::DiskUsage { ref mount_point } => assert_eq!(mount_point, "/srv"),
            other => panic!("unexpected metric: {other:?}"),
        }

        let external: AlertRule = toml::from_str(
            r#"
            name = "e"
            metric = { external = { name = "queue_depth" } }
            condition = "above"
            threshold = 1.0
            webhook_url = "u"
            "#,
        )
        .unwrap();
        match external.metric {
            AlertMetric::External { ref name } => assert_eq!(name, "queue_depth"),
            other => panic!("unexpected metric: {other:?}"),
        }
    }

    #[test]
    fn load_average_metric_uses_serde_snake_case_name() {
        // Guards the `load_average1m` spelling that once broke a live config.
        let rule: AlertRule = toml::from_str(
            r#"
            name = "l"
            metric = "load_average1m"
            condition = "above"
            threshold = 8.0
            webhook_url = "u"
            "#,
        )
        .unwrap();
        assert!(matches!(rule.metric, AlertMetric::LoadAverage1m));
    }

    #[test]
    fn service_rule_parses_states_and_defaults_cooldown() {
        let rule: ServiceAlertRule = toml::from_str(
            r#"
            name = "nginx down"
            unit = "nginx"
            alert_when = ["failed", "inactive"]
            webhook_url = "https://example/hook"
            "#,
        )
        .unwrap();

        assert_eq!(rule.unit, "nginx");
        assert_eq!(
            rule.alert_when,
            vec![ServiceState::Failed, ServiceState::Inactive]
        );
        assert_eq!(rule.cooldown_seconds, default_cooldown());
    }
}
