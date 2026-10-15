//! Characterization tests pinning the JSON shape of the snapshot endpoints.
//!
//! They go through the real router with a snapshot already stored, so the
//! response body is exactly what the dashboard and external consumers read.

use std::sync::Arc;

use async_trait::async_trait;
use axum::body::{to_bytes, Body};
use axum::http::{Request, StatusCode};
use chrono::{TimeZone, Utc};
use serde_json::{json, Value};
// `oneshot` comes from this extension trait: it drives one request through
// the router without binding a socket.
use tower::ServiceExt;

use crate::adapters::MemoryStore;
use crate::application::{Collector, DirectoryUsageService, MonitoringService};
use crate::domain::{
    Container, ContainerId, CpuMetrics, Disk, Host, LoadAverage, MemoryMetrics, NetworkInterface,
    NetworkMetrics, Process, Temperature,
};
use crate::ports::{ContainerSource, ContainerStats, HostInfo, ProcessSource, SystemSource};

use super::create_router;

type BoxErr = Box<dyn std::error::Error + Send + Sync>;

/// Every source is unreachable: the endpoints under test must answer from the
/// stored snapshot alone.
struct Unreachable;

fn unreachable() -> BoxErr {
    "source not reachable in API contract tests".into()
}

#[async_trait]
impl SystemSource for Unreachable {
    async fn get_host_info(&self) -> Result<HostInfo, BoxErr> {
        Err(unreachable())
    }
    async fn get_cpu_metrics(&self) -> Result<CpuMetrics, BoxErr> {
        Err(unreachable())
    }
    async fn get_memory_metrics(&self) -> Result<MemoryMetrics, BoxErr> {
        Err(unreachable())
    }
    async fn get_load_average(&self) -> Result<LoadAverage, BoxErr> {
        Err(unreachable())
    }
    async fn list_disks(&self) -> Result<Vec<Disk>, BoxErr> {
        Err(unreachable())
    }
    async fn list_network_interfaces(&self) -> Result<Vec<NetworkInterface>, BoxErr> {
        Err(unreachable())
    }
    async fn get_temperatures(&self) -> Result<Vec<Temperature>, BoxErr> {
        Err(unreachable())
    }
}

#[async_trait]
impl ContainerSource for Unreachable {
    async fn list_containers(&self) -> Result<Vec<Container>, BoxErr> {
        Err(unreachable())
    }
    async fn get_container_stats(&self, _id: &ContainerId) -> Result<ContainerStats, BoxErr> {
        Err(unreachable())
    }
}

#[async_trait]
impl ProcessSource for Unreachable {
    async fn list_processes(&self) -> Result<Vec<Process>, BoxErr> {
        Err(unreachable())
    }
    async fn get_top_by_cpu(&self, _n: usize) -> Result<Vec<Process>, BoxErr> {
        Err(unreachable())
    }
    async fn get_top_by_memory(&self, _n: usize) -> Result<Vec<Process>, BoxErr> {
        Err(unreachable())
    }
}

/// A snapshot exercising every field the endpoints serialize, optional ones
/// included (iowait, cache, swap, network rates).
fn snapshot() -> Host {
    let mut eth0 = NetworkMetrics::new(1000, 2000, 1, 2);
    eth0.rx_bytes_per_sec = Some(10.5);
    eth0.tx_bytes_per_sec = Some(20.25);

    let mut host = Host::new("nas".to_string())
        .with_timestamp(Utc.with_ymd_and_hms(2026, 1, 2, 3, 4, 5).unwrap())
        .with_metrics(
            3600,
            LoadAverage::new(0.5, 0.75, 1.25),
            CpuMetrics::new(40.0, 30.0, 10.0).with_iowait(2.5),
            MemoryMetrics::new(300, 1000, 700)
                .with_cache(50)
                .with_swap(5),
        );
    host.network_interfaces = vec![
        NetworkInterface::new("eth0".to_string(), true, eth0),
        NetworkInterface::new("lo".to_string(), false, NetworkMetrics::new(3, 4, 0, 0)),
    ];
    host.disks = vec![Disk::new(
        "/dev/md0".to_string(),
        "/volume1".to_string(),
        "ext4".to_string(),
        1000,
        400,
        600,
    )];
    host
}

async fn get_json(path: &str) -> Value {
    let unreachable = Arc::new(Unreachable);
    let service = Arc::new(MonitoringService::new(
        unreachable.clone(),
        unreachable.clone(),
        unreachable,
        Arc::new(MemoryStore::new(10)),
    ));
    service.store_snapshot(snapshot());
    let live = Collector::new(
        service.clone(),
        Arc::new(MemoryStore::new(10)),
        None,
        None,
        1,
        10,
    );
    let router = create_router(
        service,
        live,
        Arc::new(DirectoryUsageService::new(None)),
        15,
    );

    let response = router
        .oneshot(Request::get(path).body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    serde_json::from_slice(&body).unwrap()
}

fn expected_host() -> Value {
    json!({
        "timestamp": "2026-01-02T03:04:05+00:00",
        "hostname": "nas",
        "uptime_seconds": 3600,
        "load_average": { "one": 0.5, "five": 0.75, "fifteen": 1.25 },
        "cpu": {
            "usage_percent": 40.0,
            "user_percent": 30.0,
            "system_percent": 10.0,
            "iowait_percent": 2.5
        },
        "memory": {
            "used_bytes": 300,
            "total_bytes": 1000,
            "available_bytes": 700,
            "cached_bytes": 50,
            "swap_used_bytes": 5
        },
        "network": expected_network(),
        "temperatures": []
    })
}

fn expected_network() -> Value {
    json!([
        {
            "name": "eth0",
            "is_up": true,
            "metrics": {
                "rx_bytes": 1000,
                "tx_bytes": 2000,
                "rx_errors": 1,
                "tx_errors": 2,
                "rx_bytes_per_sec": 10.5,
                "tx_bytes_per_sec": 20.25
            }
        },
        {
            "name": "lo",
            "is_up": false,
            "metrics": { "rx_bytes": 3, "tx_bytes": 4, "rx_errors": 0, "tx_errors": 0 }
        }
    ])
}

fn expected_disks() -> Value {
    json!([
        {
            "device": "/dev/md0",
            "mount_point": "/volume1",
            "filesystem": "ext4",
            "total_bytes": 1000,
            "used_bytes": 400,
            "available_bytes": 600
        }
    ])
}

#[tokio::test]
async fn host_endpoint_serializes_the_snapshot_metrics() {
    assert_eq!(get_json("/api/host").await, expected_host());
}

#[tokio::test]
async fn disks_endpoint_lists_the_snapshot_disks() {
    let expected = json!({ "timestamp": "2026-01-02T03:04:05+00:00", "disks": expected_disks() });
    assert_eq!(get_json("/api/disks").await, expected);
}

#[tokio::test]
async fn network_endpoint_lists_the_snapshot_interfaces() {
    let expected = json!({
        "timestamp": "2026-01-02T03:04:05+00:00",
        "interfaces": expected_network()
    });
    assert_eq!(get_json("/api/network").await, expected);
}

#[tokio::test]
async fn dashboard_endpoint_aggregates_host_disks_and_network() {
    let expected = json!({
        "host": expected_host(),
        "containers": [],
        "stacks": [],
        "processes": [],
        "disks": expected_disks(),
        "network": expected_network()
    });
    assert_eq!(get_json("/api/dashboard").await, expected);
}
