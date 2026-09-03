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
    /// Margin the value must clear before the incident is declared over.
    ///
    /// `None` derives it from the threshold, which suits every metric here
    /// better than a fixed number would: 5% of 90 C is 4.5 C, 5% of a load of
    /// 12 is 0.6.
    #[serde(default)]
    pub hysteresis: Option<f64>,
    /// How loudly the incident should be announced.
    #[serde(default)]
    pub severity: Severity,
}

/// How much attention an incident deserves.
///
/// Drives the notification priority, so a full disk does not arrive looking
/// like a transient load spike.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Info,
    #[default]
    Warning,
    Critical,
}

impl AlertRule {
    /// Margin required to leave the breach, defaulting to 5% of the threshold.
    pub fn hysteresis(&self) -> f64 {
        self.hysteresis.unwrap_or(self.threshold.abs() * 0.05)
    }
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

impl AlertMetric {
    /// Human-readable name, for notifications an operator reads at 3am.
    ///
    /// # Returns
    /// The business wording, never the `Debug` rendering of the enum.
    pub fn label(&self) -> String {
        match self {
            Self::CpuUsage => "CPU usage".to_string(),
            Self::MemoryUsage => "Memory usage".to_string(),
            Self::LoadAverage1m => "Load average (1m)".to_string(),
            Self::DiskUsage { mount_point } => format!("Disk usage {mount_point}"),
            Self::Temperature { label } => format!("Temperature {label}"),
            Self::External { name } => name.clone(),
        }
    }

    /// Unit the value is expressed in, empty when dimensionless.
    pub fn unit(&self) -> &'static str {
        match self {
            Self::CpuUsage | Self::MemoryUsage | Self::DiskUsage { .. } => "%",
            Self::Temperature { .. } => "C",
            Self::LoadAverage1m | Self::External { .. } => "",
        }
    }
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

    /// Whether `current` has come back past `threshold` by at least
    /// `hysteresis`.
    ///
    /// # Arguments
    /// * `current` - the value just read
    /// * `threshold` - the level that made the rule fire
    /// * `hysteresis` - margin the value must clear before the alert resolves
    ///
    /// # Returns
    /// `true` once the breach is over. The margin exists so a value sitting on
    /// the threshold does not alternate between firing and resolved.
    pub fn has_cleared(&self, current: f64, threshold: f64, hysteresis: f64) -> bool {
        match self {
            Self::Above => current < threshold - hysteresis,
            Self::Below => current > threshold + hysteresis,
        }
    }
}

/// Whether an event opens an incident or closes it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AlertKind {
    Firing,
    Resolved,
}

/// An alert event fired when a rule triggers
#[derive(Debug, Clone, Serialize)]
pub struct AlertEvent {
    pub kind: AlertKind,
    pub severity: Severity,
    pub rule_name: String,
    pub metric: String,
    /// Unit of `current_value` and `threshold`, empty when dimensionless.
    pub unit: String,
    pub current_value: f64,
    pub threshold: f64,
    pub condition: String,
    pub hostname: String,
    pub timestamp: String,
    /// How long the breach lasted, present only on a resolution.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub breached_for_seconds: Option<u64>,
    /// Level at which the hardware itself warns, when the source publishes one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub warn_at: Option<f64>,
}

/// A metric value resolved from a snapshot, with the vendor limit when known.
///
/// Keeping the two together stops the caller from pairing a value with the
/// wrong sensor's threshold.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MetricReading {
    pub value: f64,
    /// Level the hardware flags as high, when it publishes one.
    pub warn_at: Option<f64>,
}

impl MetricReading {
    /// A reading from a source that publishes no threshold of its own.
    pub fn bare(value: f64) -> Self {
        Self {
            value,
            warn_at: None,
        }
    }
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
    use super::{
        default_cooldown, AlertCondition, AlertMetric, AlertRule, ServiceAlertRule, Severity,
    };
    use crate::domain::ServiceState;

    #[test]
    fn clearing_requires_crossing_back_past_the_hysteresis_margin() {
        let above = AlertCondition::Above;
        // Still above the threshold: obviously not cleared.
        assert!(!above.has_cleared(91.0, 90.0, 5.0));
        // Back under the threshold but inside the margin: still counted as
        // firing, so a value hovering at the limit cannot flap.
        assert!(!above.has_cleared(89.0, 90.0, 5.0));
        assert!(!above.has_cleared(85.0, 90.0, 5.0));
        // Past the margin: cleared.
        assert!(above.has_cleared(84.9, 90.0, 5.0));
    }

    #[test]
    fn below_condition_clears_upwards() {
        let below = AlertCondition::Below;
        assert!(!below.has_cleared(9.0, 10.0, 2.0));
        assert!(!below.has_cleared(11.0, 10.0, 2.0));
        assert!(below.has_cleared(12.1, 10.0, 2.0));
    }

    #[test]
    fn metrics_describe_themselves_in_business_terms() {
        assert_eq!(AlertMetric::CpuUsage.label(), "CPU usage");
        assert_eq!(AlertMetric::MemoryUsage.label(), "Memory usage");
        assert_eq!(AlertMetric::LoadAverage1m.label(), "Load average (1m)");
        assert_eq!(
            AlertMetric::DiskUsage {
                mount_point: "/volume1".to_string()
            }
            .label(),
            "Disk usage /volume1"
        );
        assert_eq!(
            AlertMetric::Temperature {
                label: "Package id 0".to_string()
            }
            .label(),
            "Temperature Package id 0"
        );
        assert_eq!(
            AlertMetric::External {
                name: "queue_depth".to_string()
            }
            .label(),
            "queue_depth"
        );
    }

    #[test]
    fn metrics_carry_their_unit() {
        assert_eq!(AlertMetric::CpuUsage.unit(), "%");
        assert_eq!(AlertMetric::MemoryUsage.unit(), "%");
        assert_eq!(
            AlertMetric::DiskUsage {
                mount_point: "/".to_string()
            }
            .unit(),
            "%"
        );
        assert_eq!(
            AlertMetric::Temperature {
                label: "x".to_string()
            }
            .unit(),
            "C"
        );
        // Load and arbitrary external metrics are dimensionless.
        assert_eq!(AlertMetric::LoadAverage1m.unit(), "");
        assert_eq!(
            AlertMetric::External {
                name: "q".to_string()
            }
            .unit(),
            ""
        );
    }

    #[test]
    fn severity_defaults_to_warning_and_parses_from_toml() {
        let default: AlertRule = toml::from_str(
            r#"
            name = "n"
            metric = "cpu_usage"
            condition = "above"
            threshold = 90.0
            webhook_url = "u"
            "#,
        )
        .unwrap();
        assert_eq!(default.severity, Severity::Warning);

        let critical: AlertRule = toml::from_str(
            r#"
            name = "n"
            metric = "cpu_usage"
            condition = "above"
            threshold = 90.0
            webhook_url = "u"
            severity = "critical"
            "#,
        )
        .unwrap();
        assert_eq!(critical.severity, Severity::Critical);
    }

    #[test]
    fn hysteresis_defaults_to_five_percent_of_the_threshold() {
        let mut rule: AlertRule = toml::from_str(
            r#"
            name = "n"
            metric = "cpu_usage"
            condition = "above"
            threshold = 90.0
            webhook_url = "u"
            "#,
        )
        .unwrap();
        assert_eq!(rule.hysteresis(), 4.5);

        // An explicit margin wins over the derived one.
        rule.hysteresis = Some(1.0);
        assert_eq!(rule.hysteresis(), 1.0);
    }

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
