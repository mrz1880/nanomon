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
        }
    }

    pub fn has_rules(&self) -> bool {
        !self.rules.is_empty() || !self.service_rules.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::AlertEvaluator;
    use crate::domain::{
        AlertEvent, ServiceAlertEvent, ServiceAlertRule, ServiceState, SystemdService,
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
