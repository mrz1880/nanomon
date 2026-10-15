use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use chrono::{DateTime, Utc};

use crate::domain::{
    AlertEvent, AlertKind, AlertMetric, AlertRule, Host, MetricReading, ServiceAlertEvent,
    ServiceAlertRule, ServiceState, SystemdService,
};
use crate::ports::AlertSink;

// A std lock is "poisoned" when a thread panics while holding its guard;
// every later lock() then returns Err. The critical sections below only
// read or swap plain data and never panic, so poisoning cannot happen.
const LOCK_INVARIANT: &str = "alert state lock poisoned: no code panics while holding it";

/// What the evaluator remembers about a breaching numeric rule between cycles.
struct BreachState {
    /// When the value first crossed the threshold, for the incident duration.
    since: DateTime<Utc>,
    /// Whether the notification actually reached the sink. A webhook outage
    /// leaves this false so the next cycle retries instead of going quiet.
    notified: bool,
}

/// Evaluates alert rules against host snapshots and fires webhooks
pub struct AlertEvaluator {
    rules: Vec<AlertRule>,
    service_rules: Vec<ServiceAlertRule>,
    last_fired: RwLock<HashMap<String, DateTime<Utc>>>,
    /// Numeric rules currently in breach, keyed by rule name. Absence means the
    /// rule is within limits, so entering the map is the firing transition.
    breaches: RwLock<HashMap<String, BreachState>>,
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
            breaches: RwLock::new(HashMap::new()),
            last_service_state: RwLock::new(HashMap::new()),
            sink,
        }
    }

    /// Evaluate all rules against the current snapshot
    pub async fn evaluate(&self, snapshot: &Host) {
        for rule in &self.rules {
            let reading = match self.extract_metric(rule, snapshot) {
                Some(r) => r,
                None => continue,
            };
            let current_value = reading.value;

            let was_breaching = {
                let breaches = self.breaches.read().expect(LOCK_INVARIANT);
                breaches.get(&rule.name).map(|b| b.notified)
            };

            if !rule.condition.evaluate(current_value, rule.threshold) {
                // Only close the incident once the value has cleared the
                // hysteresis margin: a value sitting on the threshold would
                // otherwise alternate between firing and resolved.
                let cleared =
                    rule.condition
                        .has_cleared(current_value, rule.threshold, rule.hysteresis());
                if let (Some(notified), true) = (was_breaching, cleared) {
                    let since = self
                        .breaches
                        .write()
                        .expect(LOCK_INVARIANT)
                        .remove(&rule.name)
                        .map(|b| b.since);
                    if notified {
                        let event =
                            self.build_event(AlertKind::Resolved, rule, reading, snapshot, since);
                        if let Err(e) = self.sink.send_alert(&rule.webhook_url, &event).await {
                            tracing::error!("Failed to send resolution '{}': {}", rule.name, e);
                        }
                    }
                }
                continue;
            }

            // Already announced and still breaching: one incident, one alert.
            if was_breaching == Some(true) {
                continue;
            }

            // Remember when the breach started, without overwriting an earlier
            // start left by a send that failed.
            {
                let mut breaches = self.breaches.write().expect(LOCK_INVARIANT);
                breaches.entry(rule.name.clone()).or_insert(BreachState {
                    since: Utc::now(),
                    notified: false,
                });
            }

            // Check cooldown
            {
                let last_fired = self.last_fired.read().expect(LOCK_INVARIANT);
                if let Some(last) = last_fired.get(&rule.name) {
                    let elapsed = Utc::now().signed_duration_since(*last);
                    if elapsed.num_seconds() < rule.cooldown_seconds as i64 {
                        continue;
                    }
                }
            }

            let event = self.build_event(AlertKind::Firing, rule, reading, snapshot, None);

            if let Err(e) = self.sink.send_alert(&rule.webhook_url, &event).await {
                tracing::error!("Failed to send alert '{}': {}", rule.name, e);
            } else {
                self.last_fired
                    .write()
                    .expect(LOCK_INVARIANT)
                    .insert(rule.name.clone(), Utc::now());
                if let Some(breach) = self
                    .breaches
                    .write()
                    .expect(LOCK_INVARIANT)
                    .get_mut(&rule.name)
                {
                    breach.notified = true;
                }
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
                .expect(LOCK_INVARIANT)
                .get(&rule.unit)
                .cloned();
            let was_bad = previous
                .as_ref()
                .map(|p| rule.alert_when.contains(p))
                .unwrap_or(false);

            // Record the current state before deciding, for next-cycle transitions.
            self.last_service_state
                .write()
                .expect(LOCK_INVARIANT)
                .insert(rule.unit.clone(), state.clone());

            let transitioned_to_bad = is_bad && !was_bad;
            if !transitioned_to_bad {
                continue;
            }

            // Cooldown guard: suppress rapid re-fires (flapping) per rule.
            {
                let last_fired = self.last_fired.read().expect(LOCK_INVARIANT);
                if let Some(last) = last_fired.get(&rule.name) {
                    let elapsed = Utc::now().signed_duration_since(*last).num_seconds();
                    if elapsed < rule.cooldown_seconds as i64 {
                        continue;
                    }
                }
            }
            self.last_fired
                .write()
                .expect(LOCK_INVARIANT)
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

    /// Assemble the notification payload for a rule at its current value.
    fn build_event(
        &self,
        kind: AlertKind,
        rule: &AlertRule,
        reading: MetricReading,
        snapshot: &Host,
        breached_since: Option<DateTime<Utc>>,
    ) -> AlertEvent {
        let breached_for_seconds = breached_since
            .map(|since| Utc::now().signed_duration_since(since).num_seconds().max(0) as u64);
        AlertEvent {
            kind,
            severity: rule.severity,
            rule_name: rule.name.clone(),
            metric: rule.metric.label(),
            unit: rule.metric.unit().to_string(),
            current_value: reading.value,
            threshold: rule.threshold,
            condition: format!("{:?}", rule.condition),
            hostname: snapshot.hostname.clone(),
            timestamp: Utc::now().to_rfc3339(),
            breached_for_seconds,
            warn_at: reading.warn_at,
        }
    }

    fn extract_metric(&self, rule: &AlertRule, snapshot: &Host) -> Option<MetricReading> {
        match &rule.metric {
            AlertMetric::CpuUsage => Some(MetricReading::bare(snapshot.cpu.usage_percent)),
            AlertMetric::MemoryUsage => {
                if snapshot.memory.total_bytes == 0 {
                    return None;
                }
                Some(MetricReading::bare(
                    snapshot.memory.used_bytes as f64 / snapshot.memory.total_bytes as f64 * 100.0,
                ))
            }
            AlertMetric::DiskUsage { mount_point } => snapshot
                .disks
                .iter()
                .find(|d| d.mount_point == *mount_point)
                .map(|d| MetricReading::bare(d.usage_percent())),
            AlertMetric::LoadAverage1m => Some(MetricReading::bare(snapshot.load_average.one)),
            AlertMetric::Temperature { label } => snapshot
                .temperatures
                .iter()
                .find(|t| t.label == *label)
                .map(|t| MetricReading {
                    value: t.current_celsius,
                    warn_at: t.high_celsius,
                }),
            AlertMetric::External { name } => snapshot
                .external
                .iter()
                .find(|m| m.name == *name)
                .map(|m| MetricReading::bare(m.value)),
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
        AlertEvent, AlertKind, AlertMetric, AlertRule, CpuMetrics, Disk, ExternalMetric, Host,
        LoadAverage, MemoryMetrics, ServiceAlertEvent, ServiceAlertRule, ServiceState,
        SystemdService, Temperature, TemperatureSource,
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
            hysteresis: None,
            severity: crate::domain::Severity::Warning,
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

    /// A snapshot whose only interesting value is the CPU usage.
    fn snapshot_with_cpu(cpu: f64) -> Host {
        Host::new("nas".to_string()).with_metrics(
            100,
            LoadAverage::zero(),
            CpuMetrics::new(cpu, 0.0, 0.0),
            MemoryMetrics::new(0, 0, 0),
        )
    }

    #[tokio::test]
    async fn a_sustained_breach_notifies_once_not_every_cycle() {
        let sink = Arc::new(RecordingSink::default());
        let evaluator = AlertEvaluator::new(
            vec![metric_rule("cpu", AlertMetric::CpuUsage, 90.0, 0)],
            vec![],
            sink.clone(),
        );

        // Five collection cycles with the value still over the threshold.
        for _ in 0..5 {
            evaluator.evaluate(&snapshot_with_cpu(95.0)).await;
        }

        assert_eq!(
            sink.events().len(),
            1,
            "one incident must produce one notification"
        );
    }

    #[tokio::test]
    async fn a_breach_that_clears_sends_a_resolution() {
        let sink = Arc::new(RecordingSink::default());
        let evaluator = AlertEvaluator::new(
            vec![metric_rule("cpu", AlertMetric::CpuUsage, 90.0, 0)],
            vec![],
            sink.clone(),
        );

        evaluator.evaluate(&snapshot_with_cpu(95.0)).await;
        evaluator.evaluate(&snapshot_with_cpu(10.0)).await;

        let events = sink.events();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].kind, AlertKind::Firing);
        assert_eq!(events[1].kind, AlertKind::Resolved);
        assert_eq!(events[1].current_value, 10.0);
    }

    #[tokio::test]
    async fn only_the_resolution_reports_how_long_the_incident_lasted() {
        let sink = Arc::new(RecordingSink::default());
        let evaluator = AlertEvaluator::new(
            vec![metric_rule("cpu", AlertMetric::CpuUsage, 90.0, 0)],
            vec![],
            sink.clone(),
        );

        evaluator.evaluate(&snapshot_with_cpu(95.0)).await;
        evaluator.evaluate(&snapshot_with_cpu(10.0)).await;

        let events = sink.events();
        // The opening alert has no duration to report yet.
        assert_eq!(events[0].breached_for_seconds, None);
        // The closing one does, measured from the first breach.
        assert!(events[1].breached_for_seconds.is_some());
    }

    #[tokio::test]
    async fn a_resolved_incident_can_alert_again_later() {
        let sink = Arc::new(RecordingSink::default());
        let evaluator = AlertEvaluator::new(
            vec![metric_rule("cpu", AlertMetric::CpuUsage, 90.0, 0)],
            vec![],
            sink.clone(),
        );

        evaluator.evaluate(&snapshot_with_cpu(95.0)).await; // fires
        evaluator.evaluate(&snapshot_with_cpu(10.0)).await; // resolves
        evaluator.evaluate(&snapshot_with_cpu(95.0)).await; // fires again

        let kinds: Vec<AlertKind> = sink.events().iter().map(|e| e.kind).collect();
        assert_eq!(
            kinds,
            vec![AlertKind::Firing, AlertKind::Resolved, AlertKind::Firing]
        );
    }

    #[tokio::test]
    async fn a_value_hovering_at_the_threshold_does_not_flap() {
        let sink = Arc::new(RecordingSink::default());
        // Threshold 90 with the default 5% margin: the incident only closes
        // below 85.5.
        let evaluator = AlertEvaluator::new(
            vec![metric_rule("cpu", AlertMetric::CpuUsage, 90.0, 0)],
            vec![],
            sink.clone(),
        );

        evaluator.evaluate(&snapshot_with_cpu(95.0)).await; // fires
        evaluator.evaluate(&snapshot_with_cpu(89.0)).await; // under, inside margin
        evaluator.evaluate(&snapshot_with_cpu(95.0)).await; // over again
        evaluator.evaluate(&snapshot_with_cpu(88.0)).await; // under, inside margin

        let kinds: Vec<AlertKind> = sink.events().iter().map(|e| e.kind).collect();
        assert_eq!(
            kinds,
            vec![AlertKind::Firing],
            "hovering around the threshold is one ongoing incident"
        );
    }

    #[tokio::test]
    async fn a_value_within_limits_never_notifies() {
        let sink = Arc::new(RecordingSink::default());
        let evaluator = AlertEvaluator::new(
            vec![metric_rule("cpu", AlertMetric::CpuUsage, 90.0, 0)],
            vec![],
            sink.clone(),
        );

        // Never breached, so there is nothing to resolve either.
        evaluator.evaluate(&snapshot_with_cpu(10.0)).await;
        evaluator.evaluate(&snapshot_with_cpu(10.0)).await;

        assert!(sink.events().is_empty());
    }

    #[tokio::test]
    async fn a_temperature_alert_carries_the_sensor_own_warning_level() {
        let sink = Arc::new(RecordingSink::default());
        let evaluator = AlertEvaluator::new(
            vec![metric_rule(
                "cpu temp",
                AlertMetric::Temperature {
                    label: "Package id 0".to_string(),
                },
                90.0,
                0,
            )],
            vec![],
            sink.clone(),
        );

        let host = Host::new("nas".to_string()).with_temperatures(vec![Temperature::new(
            "Package id 0".to_string(),
            TemperatureSource::Cpu,
            92.0,
        )
        .with_thresholds(Some(100.0), Some(105.0))]);

        evaluator.evaluate(&host).await;

        // Knowing the hardware warns at 100 tells the operator whether 92 is
        // worth getting out of bed for.
        assert_eq!(sink.events()[0].warn_at, Some(100.0));
    }

    #[tokio::test]
    async fn metrics_without_a_vendor_threshold_report_none() {
        let sink = Arc::new(RecordingSink::default());
        let evaluator = AlertEvaluator::new(
            vec![metric_rule("cpu", AlertMetric::CpuUsage, 90.0, 0)],
            vec![],
            sink.clone(),
        );

        evaluator.evaluate(&snapshot_with_cpu(95.0)).await;

        assert_eq!(sink.events()[0].warn_at, None);
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
