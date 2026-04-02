use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use chrono::{DateTime, Utc};

use crate::domain::{AlertEvent, AlertMetric, AlertRule, Host};
use crate::ports::AlertSink;

/// Evaluates alert rules against host snapshots and fires webhooks
pub struct AlertEvaluator {
    rules: Vec<AlertRule>,
    last_fired: RwLock<HashMap<String, DateTime<Utc>>>,
    sink: Arc<dyn AlertSink>,
}

impl AlertEvaluator {
    pub fn new(rules: Vec<AlertRule>, sink: Arc<dyn AlertSink>) -> Self {
        Self {
            rules,
            last_fired: RwLock::new(HashMap::new()),
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
        !self.rules.is_empty()
    }
}
