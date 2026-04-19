use std::fmt::Write;
use std::sync::Arc;
use std::time::Duration;

use axum::{
    debug_handler,
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::{Deserialize, Serialize};

use crate::application::MonitoringService;
use crate::domain::{Container, Host, Process, Stack, SystemdService, Temperature};

/// Custom error type that implements IntoResponse
#[derive(Debug)]
#[allow(dead_code)]
pub struct AppError(String);

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        (StatusCode::INTERNAL_SERVER_ERROR, self.0).into_response()
    }
}

impl From<Box<dyn std::error::Error + Send + Sync>> for AppError {
    fn from(err: Box<dyn std::error::Error + Send + Sync>) -> Self {
        AppError(err.to_string())
    }
}

/// Shared application state
#[derive(Clone)]
pub struct AppState {
    pub monitoring_service: Arc<MonitoringService>,
}

/// Response for /api/host
#[derive(Debug, Serialize)]
pub struct HostResponse {
    pub timestamp: String,
    pub hostname: String,
    pub uptime_seconds: u64,
    pub load_average: serde_json::Value,
    pub cpu: serde_json::Value,
    pub memory: serde_json::Value,
    pub temperatures: Vec<Temperature>,
}

impl From<&Host> for HostResponse {
    fn from(host: &Host) -> Self {
        Self {
            timestamp: host.timestamp.to_rfc3339(),
            hostname: host.hostname.clone(),
            uptime_seconds: host.uptime_seconds,
            load_average: serde_json::to_value(&host.load_average).unwrap(),
            cpu: serde_json::to_value(&host.cpu).unwrap(),
            memory: serde_json::to_value(&host.memory).unwrap(),
            temperatures: host.temperatures.clone(),
        }
    }
}

/// Response for /api/containers
#[derive(Debug, Serialize)]
pub struct ContainersResponse {
    pub timestamp: String,
    pub containers: Vec<Container>,
    pub stacks: Vec<Stack>,
}

/// Response for /api/processes
#[derive(Debug, Serialize)]
pub struct ProcessesResponse {
    pub timestamp: String,
    pub processes: Vec<Process>,
}

/// Response for /api/disks
#[derive(Debug, Serialize)]
pub struct DisksResponse {
    pub timestamp: String,
    pub disks: serde_json::Value,
}

/// Response for /api/network
#[derive(Debug, Serialize)]
pub struct NetworkResponse {
    pub timestamp: String,
    pub interfaces: serde_json::Value,
}

/// Response for /api/dashboard (aggregated)
#[derive(Debug, Serialize)]
pub struct DashboardResponse {
    pub host: HostResponse,
    pub containers: Vec<Container>,
    pub stacks: Vec<Stack>,
    pub processes: Vec<Process>,
    pub disks: serde_json::Value,
    pub network: serde_json::Value,
}

/// Response for /api/history
#[derive(Debug, Serialize)]
pub struct HistoryResponse {
    pub timestamps: Vec<String>,
    pub cpu: Vec<f64>,
    pub memory_used: Vec<u64>,
    pub memory_total: u64,
    pub load_1: Vec<f64>,
    pub load_5: Vec<f64>,
    pub load_15: Vec<f64>,
}

/// Response for /api/services
#[derive(Debug, Serialize)]
pub struct ServicesResponse {
    pub timestamp: String,
    pub services: Vec<SystemdService>,
    pub available: bool,
}

/// Query params for /api/processes
#[derive(Debug, Deserialize)]
pub struct ProcessQuery {
    #[serde(default = "default_sort")]
    pub sort: String,
    #[serde(default = "default_limit")]
    pub limit: usize,
}

/// Query params for /api/history
#[derive(Debug, Deserialize)]
pub struct HistoryQuery {
    /// Duration in seconds (default: 3600 = 1 hour)
    #[serde(default = "default_history_duration")]
    pub duration: u64,
}

fn default_sort() -> String {
    "cpu".to_string()
}

fn default_limit() -> usize {
    20
}

fn default_history_duration() -> u64 {
    3600
}

/// Handler for GET /api/health
pub async fn health_handler() -> (StatusCode, Json<serde_json::Value>) {
    (
        StatusCode::OK,
        Json(serde_json::json!({
            "status": "healthy",
            "service": "nanomon",
            "version": env!("CARGO_PKG_VERSION")
        })),
    )
}

/// Handler for GET /api/host
#[debug_handler]
pub async fn host_handler(State(state): State<AppState>) -> Response {
    match state.monitoring_service.collect_all().await {
        Ok(host) => (StatusCode::OK, Json(HostResponse::from(&host))).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

/// Handler for GET /api/containers
pub async fn containers_handler(State(state): State<AppState>) -> Response {
    let containers = match state.monitoring_service.get_containers().await {
        Ok(c) => c,
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    };

    let stacks = match state.monitoring_service.get_stacks().await {
        Ok(s) => s,
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    };

    (
        StatusCode::OK,
        Json(ContainersResponse {
            timestamp: chrono::Utc::now().to_rfc3339(),
            containers,
            stacks,
        }),
    )
        .into_response()
}

/// Handler for GET /api/processes
pub async fn processes_handler(
    State(state): State<AppState>,
    Query(params): Query<ProcessQuery>,
) -> Response {
    let result = match params.sort.as_str() {
        "memory" => {
            state
                .monitoring_service
                .get_top_processes_by_memory(params.limit)
                .await
        }
        _ => {
            state
                .monitoring_service
                .get_top_processes_by_cpu(params.limit)
                .await
        }
    };

    match result {
        Ok(processes) => (
            StatusCode::OK,
            Json(ProcessesResponse {
                timestamp: chrono::Utc::now().to_rfc3339(),
                processes,
            }),
        )
            .into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

/// Handler for GET /api/disks
#[debug_handler]
pub async fn disks_handler(State(state): State<AppState>) -> Response {
    match state.monitoring_service.collect_all().await {
        Ok(host) => (
            StatusCode::OK,
            Json(DisksResponse {
                timestamp: host.timestamp.to_rfc3339(),
                disks: serde_json::to_value(&host.disks).unwrap(),
            }),
        )
            .into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

/// Handler for GET /api/network
#[debug_handler]
pub async fn network_handler(State(state): State<AppState>) -> Response {
    match state.monitoring_service.collect_all().await {
        Ok(host) => (
            StatusCode::OK,
            Json(NetworkResponse {
                timestamp: host.timestamp.to_rfc3339(),
                interfaces: serde_json::to_value(&host.network_interfaces).unwrap(),
            }),
        )
            .into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

/// Handler for GET /api/dashboard (aggregated endpoint)
#[debug_handler]
pub async fn dashboard_handler(State(state): State<AppState>) -> Response {
    let host = match state.monitoring_service.collect_all().await {
        Ok(h) => h,
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    };

    let stacks = match state.monitoring_service.get_stacks().await {
        Ok(s) => s,
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    };

    let processes = match state.monitoring_service.get_top_processes_by_cpu(20).await {
        Ok(p) => p,
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    };

    (
        StatusCode::OK,
        Json(DashboardResponse {
            host: HostResponse::from(&host),
            containers: host.containers.clone(),
            stacks,
            processes,
            disks: serde_json::to_value(&host.disks).unwrap(),
            network: serde_json::to_value(&host.network_interfaces).unwrap(),
        }),
    )
        .into_response()
}

/// Handler for GET /api/history
#[debug_handler]
pub async fn history_handler(
    State(state): State<AppState>,
    Query(params): Query<HistoryQuery>,
) -> Response {
    let history = state
        .monitoring_service
        .get_history(Duration::from_secs(params.duration));

    if history.is_empty() {
        return (
            StatusCode::OK,
            Json(HistoryResponse {
                timestamps: Vec::new(),
                cpu: Vec::new(),
                memory_used: Vec::new(),
                memory_total: 0,
                load_1: Vec::new(),
                load_5: Vec::new(),
                load_15: Vec::new(),
            }),
        )
            .into_response();
    }

    let memory_total = history.last().map(|h| h.memory.total_bytes).unwrap_or(0);

    let response = HistoryResponse {
        timestamps: history.iter().map(|h| h.timestamp.to_rfc3339()).collect(),
        cpu: history.iter().map(|h| h.cpu.usage_percent).collect(),
        memory_used: history.iter().map(|h| h.memory.used_bytes).collect(),
        memory_total,
        load_1: history.iter().map(|h| h.load_average.one).collect(),
        load_5: history.iter().map(|h| h.load_average.five).collect(),
        load_15: history.iter().map(|h| h.load_average.fifteen).collect(),
    };

    (StatusCode::OK, Json(response)).into_response()
}

/// Handler for GET /api/services
#[debug_handler]
pub async fn services_handler(State(state): State<AppState>) -> Response {
    let available = state.monitoring_service.has_services();
    let services = match state.monitoring_service.get_services().await {
        Ok(s) => s,
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    };

    (
        StatusCode::OK,
        Json(ServicesResponse {
            timestamp: chrono::Utc::now().to_rfc3339(),
            services,
            available,
        }),
    )
        .into_response()
}

/// Handler for GET /api/containers/:name
#[debug_handler]
pub async fn container_detail_handler(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> Response {
    let containers = match state.monitoring_service.get_containers().await {
        Ok(c) => c,
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    };

    match containers.into_iter().find(|c| c.name == name) {
        Some(container) => (StatusCode::OK, Json(container)).into_response(),
        None => (
            StatusCode::NOT_FOUND,
            format!("Container '{}' not found", name),
        )
            .into_response(),
    }
}

/// Handler for GET /metrics (Prometheus text exposition format)
#[debug_handler]
pub async fn prometheus_handler(State(state): State<AppState>) -> Response {
    // Try latest snapshot from store first, fall back to live collection
    let host = match state.monitoring_service.get_latest_snapshot() {
        Some(h) => (*h).clone(),
        None => match state.monitoring_service.collect_all().await {
            Ok(h) => h,
            Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
        },
    };

    let mut output = String::with_capacity(4096);

    // Host metrics
    write_metric(
        &mut output,
        "nanomon_host_cpu_usage_percent",
        "gauge",
        "Host CPU usage percentage",
        host.cpu.usage_percent,
        &[],
    );
    write_metric(
        &mut output,
        "nanomon_host_memory_used_bytes",
        "gauge",
        "Host memory used in bytes",
        host.memory.used_bytes as f64,
        &[],
    );
    write_metric(
        &mut output,
        "nanomon_host_memory_total_bytes",
        "gauge",
        "Host total memory in bytes",
        host.memory.total_bytes as f64,
        &[],
    );
    write_metric(
        &mut output,
        "nanomon_host_memory_available_bytes",
        "gauge",
        "Host available memory in bytes",
        host.memory.available_bytes as f64,
        &[],
    );
    write_metric(
        &mut output,
        "nanomon_host_uptime_seconds",
        "gauge",
        "Host uptime in seconds",
        host.uptime_seconds as f64,
        &[],
    );

    // Load average
    let _ = writeln!(
        output,
        "# HELP nanomon_host_load_average System load average"
    );
    let _ = writeln!(output, "# TYPE nanomon_host_load_average gauge");
    let _ = writeln!(
        output,
        "nanomon_host_load_average{{period=\"1m\"}} {}",
        host.load_average.one
    );
    let _ = writeln!(
        output,
        "nanomon_host_load_average{{period=\"5m\"}} {}",
        host.load_average.five
    );
    let _ = writeln!(
        output,
        "nanomon_host_load_average{{period=\"15m\"}} {}",
        host.load_average.fifteen
    );

    // Disks
    for disk in &host.disks {
        let labels = [
            ("mount", disk.mount_point.as_str()),
            ("device", disk.device.as_str()),
        ];
        write_metric(
            &mut output,
            "nanomon_disk_used_bytes",
            "gauge",
            "Disk used bytes",
            disk.used_bytes as f64,
            &labels,
        );
        write_metric(
            &mut output,
            "nanomon_disk_total_bytes",
            "gauge",
            "Disk total bytes",
            disk.total_bytes as f64,
            &labels,
        );
    }

    // Network interfaces
    for iface in &host.network_interfaces {
        let labels = [("interface", iface.name.as_str())];
        write_metric(
            &mut output,
            "nanomon_network_rx_bytes_total",
            "counter",
            "Network received bytes",
            iface.metrics.rx_bytes as f64,
            &labels,
        );
        write_metric(
            &mut output,
            "nanomon_network_tx_bytes_total",
            "counter",
            "Network transmitted bytes",
            iface.metrics.tx_bytes as f64,
            &labels,
        );
    }

    // Containers
    for container in &host.containers {
        if container.state != crate::domain::ContainerState::Running {
            continue;
        }
        let stack_label = container.stack.as_deref().unwrap_or("");
        let labels = [("name", container.name.as_str()), ("stack", stack_label)];
        write_metric(
            &mut output,
            "nanomon_container_cpu_usage_percent",
            "gauge",
            "Container CPU usage",
            container.cpu.usage_percent,
            &labels,
        );
        write_metric(
            &mut output,
            "nanomon_container_memory_used_bytes",
            "gauge",
            "Container memory used",
            container.memory.used_bytes as f64,
            &labels,
        );
    }

    // Temperatures
    for temp in &host.temperatures {
        let source_str = match temp.source {
            crate::domain::TemperatureSource::Cpu => "cpu",
            crate::domain::TemperatureSource::Disk => "disk",
            crate::domain::TemperatureSource::Other => "other",
        };
        let labels = [("label", temp.label.as_str()), ("source", source_str)];
        write_metric(
            &mut output,
            "nanomon_temperature_celsius",
            "gauge",
            "Temperature in Celsius",
            temp.current_celsius,
            &labels,
        );
    }

    (
        StatusCode::OK,
        [(
            axum::http::header::CONTENT_TYPE,
            "text/plain; version=0.0.4; charset=utf-8",
        )],
        output,
    )
        .into_response()
}

fn write_metric(
    output: &mut String,
    name: &str,
    metric_type: &str,
    help: &str,
    value: f64,
    labels: &[(&str, &str)],
) {
    let _ = writeln!(output, "# HELP {} {}", name, help);
    let _ = writeln!(output, "# TYPE {} {}", name, metric_type);
    if labels.is_empty() {
        let _ = writeln!(output, "{} {}", name, value);
    } else {
        let label_str: String = labels
            .iter()
            .map(|(k, v)| format!("{}=\"{}\"", k, v.replace('\\', "\\\\").replace('"', "\\\"")))
            .collect::<Vec<_>>()
            .join(",");
        let _ = writeln!(output, "{}{{{}}} {}", name, label_str, value);
    }
}
