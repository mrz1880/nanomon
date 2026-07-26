use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use chrono::{DateTime, Utc};
use tokio::sync::broadcast;
use tokio::time::interval;
use tracing::error;

use crate::domain::Host;
use crate::ports::MetricStore;

use super::{AlertEvaluator, MonitoringService};

/// Snapshot of cumulative network counters, used to derive throughput rates.
struct NetSample {
    at: DateTime<Utc>,
    /// interface name -> (rx_bytes, tx_bytes)
    ifaces: HashMap<String, (u64, u64)>,
}

/// Compute a byte/second rate from two cumulative counter readings.
///
/// Returns `None` when the counter went backwards (device reset / wrap), so the
/// UI shows a gap rather than a nonsensical spike.
fn rate(current: u64, previous: u64, dt_secs: f64) -> Option<f64> {
    if dt_secs <= 0.0 || current < previous {
        return None;
    }
    Some((current - previous) as f64 / dt_secs)
}

/// Two-speed background collector.
///
/// - **fast loop** (`fast_interval`, ~1s): collects host metrics only, derives
///   network throughput rates, stores them in a small high-resolution ring
///   buffer and broadcasts them to live SSE clients.
/// - **slow loop** (`slow_interval`, ~10s): collects disks, temperatures,
///   containers and processes, merges them with the latest fast host snapshot,
///   evaluates alerts and stores the full snapshot for history / Prometheus.
///
/// The fast loop is the sole reader of the host CPU delta baseline, keeping the
/// per-second `usage_percent` accurate.
pub struct Collector {
    service: Arc<MonitoringService>,
    hires_store: Arc<dyn MetricStore>,
    tx: broadcast::Sender<Arc<Host>>,
    alert: Option<Arc<AlertEvaluator>>,
    net_prev: Mutex<Option<NetSample>>,
    fast_interval: u64,
    slow_interval: u64,
}

impl Collector {
    pub fn new(
        service: Arc<MonitoringService>,
        hires_store: Arc<dyn MetricStore>,
        alert: Option<Arc<AlertEvaluator>>,
        fast_interval: u64,
        slow_interval: u64,
    ) -> Arc<Self> {
        let (tx, _rx) = broadcast::channel(16);
        Arc::new(Self {
            service,
            hires_store,
            tx,
            alert,
            net_prev: Mutex::new(None),
            fast_interval,
            slow_interval,
        })
    }

    /// Subscribe to the live host stream (one receiver per SSE client).
    pub fn subscribe(&self) -> broadcast::Receiver<Arc<Host>> {
        self.tx.subscribe()
    }

    /// Access the high-resolution store (used to seed live charts on load).
    pub fn hires_store(&self) -> Arc<dyn MetricStore> {
        self.hires_store.clone()
    }

    /// Spawn the fast and slow collection loops.
    pub fn spawn(self: Arc<Self>) {
        let fast = self.clone();
        tokio::spawn(async move {
            let mut ticker = interval(Duration::from_secs(fast.fast_interval));
            loop {
                ticker.tick().await;
                fast.tick_fast().await;
            }
        });

        let slow = self;
        tokio::spawn(async move {
            let mut ticker = interval(Duration::from_secs(slow.slow_interval));
            loop {
                ticker.tick().await;
                slow.tick_slow().await;
            }
        });
    }

    async fn tick_fast(&self) {
        let mut host = match self.service.collect_host().await {
            Ok(h) => h,
            Err(e) => {
                error!("Fast host collection failed: {}", e);
                return;
            }
        };

        self.apply_network_rates(&mut host);
        self.hires_store.store(host.clone());
        // Ignore send errors: no subscribers just means nobody is watching live.
        let _ = self.tx.send(Arc::new(host));
    }

    async fn tick_slow(&self) {
        let resources = match self.service.collect_slow().await {
            Ok(r) => r,
            Err(e) => {
                error!("Slow resource collection failed: {}", e);
                return;
            }
        };

        // Reuse the latest fast host snapshot so /proc/stat is read once per
        // second by the fast loop only. Fall back to a direct collect on the
        // very first tick, before any fast snapshot exists.
        let base = match self.hires_store.get_latest() {
            Some(h) => (*h).clone(),
            None => match self.service.collect_host().await {
                Ok(h) => h,
                Err(e) => {
                    error!("Slow base host collection failed: {}", e);
                    return;
                }
            },
        };

        let merged = base
            .with_disks(resources.disks)
            .with_temperatures(resources.temperatures)
            .with_containers(resources.containers)
            .with_processes(resources.processes)
            .with_timestamp(Utc::now());

        if let Some(alert) = &self.alert {
            alert.evaluate(&merged).await;
        }

        self.service.store_snapshot(merged);
    }

    /// Fill in per-interface throughput rates by diffing against the previous
    /// fast sample, then record the current counters for the next tick.
    fn apply_network_rates(&self, host: &mut Host) {
        let now = host.timestamp;
        let mut guard = self.net_prev.lock().unwrap();

        if let Some(prev) = guard.as_ref() {
            let dt = (now - prev.at).num_milliseconds() as f64 / 1000.0;
            for iface in &mut host.network_interfaces {
                if let Some(&(prev_rx, prev_tx)) = prev.ifaces.get(&iface.name) {
                    if let (Some(rx), Some(tx)) = (
                        rate(iface.metrics.rx_bytes, prev_rx, dt),
                        rate(iface.metrics.tx_bytes, prev_tx, dt),
                    ) {
                        iface.metrics.rx_bytes_per_sec = Some(rx);
                        iface.metrics.tx_bytes_per_sec = Some(tx);
                    }
                }
            }
        }

        let ifaces = host
            .network_interfaces
            .iter()
            .map(|i| (i.name.clone(), (i.metrics.rx_bytes, i.metrics.tx_bytes)))
            .collect();
        *guard = Some(NetSample { at: now, ifaces });
    }
}

#[cfg(test)]
mod tests {
    use super::rate;

    #[test]
    fn rate_computes_bytes_per_second() {
        assert_eq!(rate(2000, 1000, 2.0), Some(500.0));
    }

    #[test]
    fn rate_zero_delta_is_zero() {
        assert_eq!(rate(1000, 1000, 1.0), Some(0.0));
    }

    #[test]
    fn rate_counter_reset_yields_none() {
        // Counter went backwards (device reset / wrap) -> no bogus spike.
        assert_eq!(rate(500, 1000, 1.0), None);
    }

    #[test]
    fn rate_non_positive_dt_yields_none() {
        assert_eq!(rate(2000, 1000, 0.0), None);
        assert_eq!(rate(2000, 1000, -1.0), None);
    }
}
