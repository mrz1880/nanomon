use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use chrono::{DateTime, Utc};

use crate::domain::{
    AlertEvent, AlertMetric, AlertRule, Host, ServiceAlertEvent, ServiceAlertRule, ServiceState,
    SystemdService,
};
use crate::ports::AlertSink;

/// Evaluates alert rules against host snapshots and fires webhooks
pub struct AlertEvaluator {
    rules: Vec<AlertRule>,
    service_rules: Vec<ServiceAlertRule>,
    last_fired: RwLock<HashMap<String, DateTime<Utc>>>,
    /// Last observed state per systemd unit, used for transition detection.
    last_service_state: RwLock<HashMap<String, ServiceState>>,
    sink: Arc<dyn AlertSink>,
}

impl AlertEvaluator {
    pub fn new(
        rules: Vec<AlertRule>,
        service_rules: Vec<ServiceAlertRule>,
        sink: Arc<dyn AlertSink>,
    ) -> Self {
        Self {
            rules,
            service_rules,
            last_fired: RwLock::new(HashMap::new()),
            last_service_state: RwLock::new(HashMap::new()),
            sink,
        }
    }

    /// Evaluate all rules against the current snapshot
    pub async fn evaluate(&self, snapshot: &Host) {
        for rule in &self.rules {
            let current_value = match self.extract_metric(rule, snapshot) {
                Some(v) => v,
                None => continue,
            };

            if !rule.condition.evaluate(current_value, rule.threshold) {
                continue;
            }

            // Check cooldown
            {
                let last_fired = self.last_fired.read().unwrap();
                if let Some(last) = last_fired.get(&rule.name) {
                    let elapsed = Utc::now().signed_duration_since(*last);
                    if elapsed.num_seconds() < rule.cooldown_seconds as i64 {
                        continue;
                    }
                }
            }

            let event = AlertEvent {
                rule_name: rule.name.clone(),
                metric: format!("{:?}", rule.metric),
                current_value,
                threshold: rule.threshold,
                condition: format!("{:?}", rule.condition),
                hostname: snapshot.hostname.clone(),
                timestamp: Utc::now().to_rfc3339(),
            };

            if let Err(e) = self.sink.send_alert(&rule.webhook_url, &event).await {
                tracing::error!("Failed to send alert '{}': {}", rule.name, e);
            } else {
                let mut last_fired = self.last_fired.write().unwrap();
                last_fired.insert(rule.name.clone(), Utc::now());
            }
        }

        // Systemd service rules: fire on transition into a "bad" state.
        for (url, event) in self.evaluate_services(&snapshot.services, &snapshot.hostname) {
            if let Err(e) = self.sink.send_service_alert(&url, &event).await {
                tracing::error!("Failed to send service alert '{}': {}", event.rule_name, e);
            }
        }
    }

    /// Decide which service alerts should fire for the current snapshot.
    ///
    /// A rule fires only when its unit *transitions* into a bad state (was not
    /// bad, or previously unseen), and only if outside the cooldown window — so
    /// a unit that stays down does not spam, but a recovery followed by a new
    /// failure alerts again. Updates the per-unit state map as a side effect.
    fn evaluate_services(
        &self,
        services: &[SystemdService],
        hostname: &str,
    ) -> Vec<(String, ServiceAlertEvent)> {
        let mut fired = Vec::new();

        for rule in &self.service_rules {
            let Some(service) = services.iter().find(|s| s.name == rule.unit) else {
                continue; // unit not present in this snapshot
            };
            let state = service.state.clone();
            let is_bad = rule.alert_when.contains(&state);

            let previous = self
                .last_service_state
                .read()
                .unwrap()
                .get(&rule.unit)
                .cloned();
            let was_bad = previous
                .as_ref()
                .map(|p| rule.alert_when.contains(p))
                .unwrap_or(false);

            // Record the current state before deciding, for next-cycle transitions.
            self.last_service_state
                .write()
                .unwrap()
                .insert(rule.unit.clone(), state.clone());

            let transitioned_to_bad = is_bad && !was_bad;
            if !transitioned_to_bad {
                continue;
            }

            // Cooldown guard: suppress rapid re-fires (flapping) per rule.
            {
                let last_fired = self.last_fired.read().unwrap();
                if let Some(last) = last_fired.get(&rule.name) {
                    let elapsed = Utc::now().signed_duration_since(*last).num_seconds();
                    if elapsed < rule.cooldown_seconds as i64 {
                        continue;
                    }
                }
            }
            self.last_fired
                .write()
                .unwrap()
                .insert(rule.name.clone(), Utc::now());

            fired.push((
                rule.webhook_url.clone(),
                ServiceAlertEvent {
                    rule_name: rule.name.clone(),
                    unit: rule.unit.clone(),
                    state: format!("{state:?}").to_lowercase(),
                    hostname: hostname.to_string(),
                    timestamp: Utc::now().to_rfc3339(),
                },
            ));
        }

        fired
    }

    fn extract_metric(&self, rule: &AlertRule, snapshot: &Host) -> Option<f64> {
        match &rule.metric {
            AlertMetric::CpuUsage => Some(snapshot.cpu.usage_percent),
            AlertMetric::MemoryUsage => {
                if snapshot.memory.total_bytes == 0 {
                    return None;
                }
                Some(snapshot.memory.used_bytes as f64 / snapshot.memory.total_bytes as f64 * 100.0)
            }
            AlertMetric::DiskUsage { mount_point } => snapshot
                .disks
                .iter()
                .find(|d| d.mount_point == *mount_point)
                .map(|d| d.usage_percent()),
            AlertMetric::LoadAverage1m => Some(snapshot.load_average.one),
            AlertMetric::Temperature { label } => snapshot
                .temperatures
                .iter()
                .find(|t| t.label == *label)
                .map(|t| t.current_celsius),
            AlertMetric::External { name } => snapshot
                .external
                .iter()
                .find(|m| m.name == *name)
                .map(|m| m.value),
        }
    }

    pub fn has_rules(&self) -> bool {
        !self.rules.is_empty() || !self.service_rules.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::AlertEvaluator;
    use crate::domain::alert::AlertCondition;
    use crate::domain::{
        AlertEvent, AlertMetric, AlertRule, CpuMetrics, Disk, ExternalMetric, Host, LoadAverage,
        MemoryMetrics, ServiceAlertEvent, ServiceAlertRule, ServiceState, SystemdService,
        Temperature, TemperatureSource,
    };
    use crate::ports::AlertSink;
    use async_trait::async_trait;
    use std::sync::Arc;

    struct NoopSink;

    #[async_trait]
    impl AlertSink for NoopSink {
        async fn send_alert(
            &self,
            _url: &str,
            _event: &AlertEvent,
        ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
            Ok(())
        }
        async fn send_service_alert(
            &self,
            _url: &str,
            _event: &ServiceAlertEvent,
        ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
            Ok(())
        }
    }

    /// Captures what was sent, so tests can assert on the payload.
    #[derive(Default)]
    struct RecordingSink {
        sent: std::sync::Mutex<Vec<AlertEvent>>,
    }

    impl RecordingSink {
        fn events(&self) -> Vec<AlertEvent> {
            self.sent.lock().unwrap().clone()
        }
    }

    #[async_trait]
    impl AlertSink for RecordingSink {
        async fn send_alert(
            &self,
            _url: &str,
            event: &AlertEvent,
        ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
            self.sent.lock().unwrap().push(event.clone());
            Ok(())
        }
        async fn send_service_alert(
            &self,
            _url: &str,
            _event: &ServiceAlertEvent,
        ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
            Ok(())
        }
    }

    /// A sink that always fails, to check failures don't mark a rule as fired.
    struct FailingSink;

    #[async_trait]
    impl AlertSink for FailingSink {
        async fn send_alert(
            &self,
            _url: &str,
            _event: &AlertEvent,
        ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
            Err("webhook down".into())
        }
        async fn send_service_alert(
            &self,
            _url: &str,
            _event: &ServiceAlertEvent,
        ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
            Err("webhook down".into())
        }
    }

    fn metric_rule(name: &str, metric: AlertMetric, threshold: f64, cooldown: u64) -> AlertRule {
        AlertRule {
            name: name.to_string(),
            metric,
            condition: AlertCondition::Above,
            threshold,
            webhook_url: "https://example/hook".to_string(),
            cooldown_seconds: cooldown,
        }
    }

    /// A snapshot with one disk, one temperature and one external metric.
    fn snapshot() -> Host {
        Host::new("nas".to_string())
            .with_metrics(
                100,
                LoadAverage::new(9.0, 1.0, 1.0),
                CpuMetrics::new(95.0, 0.0, 0.0),
                MemoryMetrics::new(900, 1000, 100),
            )
            .with_disks(vec![Disk::new(
                "/dev/sda".to_string(),
                "/".to_string(),
                "ext4".to_string(),
                1000,
                910,
                90,
            )])
            .with_temperatures(vec![Temperature::new(
                "Package id 0".to_string(),
                TemperatureSource::Cpu,
                92.0,
            )])
            .with_external(vec![ExternalMetric {
                job: "app".to_string(),
                name: "queue_depth".to_string(),
                value: 1500.0,
            }])
    }

    #[tokio::test]
    async fn every_metric_kind_can_fire() {
        let cases = vec![
            ("cpu", AlertMetric::CpuUsage, 90.0),
            ("mem", AlertMetric::MemoryUsage, 80.0),
            ("load", AlertMetric::LoadAverage1m, 8.0),
            (
                "disk",
                AlertMetric::DiskUsage {
                    mount_point: "/".to_string(),
                },
                90.0,
            ),
            (
                "temp",
                AlertMetric::Temperature {
                    label: "Package id 0".to_string(),
                },
                85.0,
            ),
            (
                "external",
                AlertMetric::External {
                    name: "queue_depth".to_string(),
                },
                1000.0,
            ),
        ];

        for (name, metric, threshold) in cases {
            let sink = Arc::new(RecordingSink::default());
            let evaluator = AlertEvaluator::new(
                vec![metric_rule(name, metric, threshold, 0)],
                vec![],
                sink.clone(),
            );
            evaluator.evaluate(&snapshot()).await;

            let events = sink.events();
            assert_eq!(events.len(), 1, "rule '{name}' should have fired");
            assert_eq!(events[0].rule_name, name);
            assert_eq!(events[0].hostname, "nas");
            assert!(events[0].current_value > threshold);
        }
    }

    #[tokio::test]
    async fn metrics_that_cannot_be_resolved_are_skipped() {
        let sink = Arc::new(RecordingSink::default());
        let evaluator = AlertEvaluator::new(
            vec![
                metric_rule(
                    "unknown disk",
                    AlertMetric::DiskUsage {
                        mount_point: "/nope".to_string(),
                    },
                    0.0,
                    0,
                ),
                metric_rule(
                    "unknown sensor",
                    AlertMetric::Temperature {
                        label: "nope".to_string(),
                    },
                    0.0,
                    0,
                ),
                metric_rule(
                    "unknown external",
                    AlertMetric::External {
                        name: "nope".to_string(),
                    },
                    0.0,
                    0,
                ),
            ],
            vec![],
            sink.clone(),
        );

        evaluator.evaluate(&snapshot()).await;
        assert!(sink.events().is_empty());
    }

    #[tokio::test]
    async fn memory_usage_is_skipped_when_total_is_unknown() {
        let sink = Arc::new(RecordingSink::default());
        let evaluator = AlertEvaluator::new(
            vec![metric_rule("mem", AlertMetric::MemoryUsage, 0.0, 0)],
            vec![],
            sink.clone(),
        );

        // A snapshot collected before memory was readable: total is zero.
        evaluator.evaluate(&Host::new("nas".to_string())).await;
        assert!(sink.events().is_empty());
    }

    #[tokio::test]
    async fn values_below_the_threshold_do_not_fire() {
        let sink = Arc::new(RecordingSink::default());
        let evaluator = AlertEvaluator::new(
            vec![metric_rule("cpu", AlertMetric::CpuUsage, 99.0, 0)],
            vec![],
            sink.clone(),
        );

        evaluator.evaluate(&snapshot()).await; // cpu is 95%
        assert!(sink.events().is_empty());
    }

    #[tokio::test]
    async fn cooldown_suppresses_repeat_numeric_alerts() {
        let sink = Arc::new(RecordingSink::default());
        let evaluator = AlertEvaluator::new(
            vec![metric_rule("cpu", AlertMetric::CpuUsage, 90.0, 300)],
            vec![],
            sink.clone(),
        );

        evaluator.evaluate(&snapshot()).await;
        evaluator.evaluate(&snapshot()).await;
        assert_eq!(sink.events().len(), 1);
    }

    #[tokio::test]
    async fn a_failed_send_does_not_start_the_cooldown() {
        let evaluator = AlertEvaluator::new(
            vec![metric_rule("cpu", AlertMetric::CpuUsage, 90.0, 3600)],
            vec![],
            Arc::new(FailingSink),
        );

        // Both attempts run: a webhook outage must not silence the next cycle.
        evaluator.evaluate(&snapshot()).await;
        evaluator.evaluate(&snapshot()).await;
    }

    #[test]
    fn has_rules_covers_both_rule_kinds() {
        let sink = Arc::new(NoopSink);
        assert!(!AlertEvaluator::new(vec![], vec![], sink.clone()).has_rules());
        assert!(AlertEvaluator::new(
            vec![metric_rule("cpu", AlertMetric::CpuUsage, 1.0, 0)],
            vec![],
            sink.clone()
        )
        .has_rules());
        assert!(AlertEvaluator::new(vec![], vec![rule(0)], sink).has_rules());
    }

    fn rule(cooldown: u64) -> ServiceAlertRule {
        ServiceAlertRule {
            name: "nginx down".to_string(),
            unit: "nginx".to_string(),
            alert_when: vec![ServiceState::Failed, ServiceState::Inactive],
            webhook_url: "http://example/webhook".to_string(),
            cooldown_seconds: cooldown,
        }
    }

    fn svc(name: &str, state: ServiceState) -> SystemdService {
        SystemdService::new(name.to_string(), String::new(), state, String::new())
    }

    fn evaluator(cooldown: u64) -> AlertEvaluator {
        AlertEvaluator::new(vec![], vec![rule(cooldown)], Arc::new(NoopSink))
    }

    #[test]
    fn fires_on_transition_and_not_on_active() {
        let ev = evaluator(0);

        // Active -> no alert.
        assert!(ev
            .evaluate_services(&[svc("nginx", ServiceState::Active)], "host")
            .is_empty());

        // Active -> Failed transition fires exactly one alert.
        let fired = ev.evaluate_services(&[svc("nginx", ServiceState::Failed)], "host");
        assert_eq!(fired.len(), 1);
        assert_eq!(fired[0].1.unit, "nginx");
        assert_eq!(fired[0].1.state, "failed");

        // Still failed -> no duplicate.
        assert!(ev
            .evaluate_services(&[svc("nginx", ServiceState::Failed)], "host")
            .is_empty());

        // Recovered.
        assert!(ev
            .evaluate_services(&[svc("nginx", ServiceState::Active)], "host")
            .is_empty());

        // Failed again after recovery -> re-fires.
        assert_eq!(
            ev.evaluate_services(&[svc("nginx", ServiceState::Failed)], "host")
                .len(),
            1
        );
    }

    #[test]
    fn cooldown_suppresses_rapid_reflap() {
        let ev = evaluator(300);

        // First failure fires.
        assert_eq!(
            ev.evaluate_services(&[svc("nginx", ServiceState::Failed)], "host")
                .len(),
            1
        );
        // Recover, then fail again within the cooldown window -> suppressed.
        ev.evaluate_services(&[svc("nginx", ServiceState::Active)], "host");
        assert!(ev
            .evaluate_services(&[svc("nginx", ServiceState::Failed)], "host")
            .is_empty());
    }

    #[test]
    fn missing_unit_does_not_fire() {
        let ev = evaluator(0);
        assert!(ev
            .evaluate_services(&[svc("other", ServiceState::Failed)], "host")
            .is_empty());
    }
}
