use std::sync::Arc;

use axum::{extract::{Query, State}, http::StatusCode, Json};
use serde::{Deserialize, Serialize};

use crate::application::MonitoringService;
use crate::domain::{Container, Host, Process, Stack};

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

/// Query params for /api/processes
#[derive(Debug, Deserialize)]
pub struct ProcessQuery {
    #[serde(default = "default_sort")]
    pub sort: String,
    #[serde(default = "default_limit")]
    pub limit: usize,
}

fn default_sort() -> String {
    "cpu".to_string()
}

fn default_limit() -> usize {
    20
}

/// Handler for GET /api/health
pub async fn health_handler() -> (StatusCode, Json<serde_json::Value>) {
    (
        StatusCode::OK,
        Json(serde_json::json!({
            "status": "healthy",
            "service": "nanomon"
        })),
    )
}

/// Handler for GET /api/host
pub async fn host_handler(
    State(state): State<AppState>,
) -> Result<Json<HostResponse>, (StatusCode, String)> {
    let host = state
        .monitoring_service
        .collect_all()
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok(Json(HostResponse::from(&host)))
}

/// Handler for GET /api/containers
pub async fn containers_handler(
    State(state): State<AppState>,
) -> Result<Json<ContainersResponse>, (StatusCode, String)> {
    let containers = state
        .monitoring_service
        .get_containers()
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let stacks = state
        .monitoring_service
        .get_stacks()
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok(Json(ContainersResponse {
        timestamp: chrono::Utc::now().to_rfc3339(),
        containers,
        stacks,
    }))
}

/// Handler for GET /api/processes
pub async fn processes_handler(
    State(state): State<AppState>,
    Query(params): Query<ProcessQuery>,
) -> Result<Json<ProcessesResponse>, (StatusCode, String)> {
    let processes = match params.sort.as_str() {
        "memory" => state.monitoring_service.get_top_processes_by_memory(params.limit),
        _ => state.monitoring_service.get_top_processes_by_cpu(params.limit),
    }
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok(Json(ProcessesResponse {
        timestamp: chrono::Utc::now().to_rfc3339(),
        processes,
    }))
}

/// Handler for GET /api/disks
pub async fn disks_handler(
    State(state): State<AppState>,
) -> Result<Json<DisksResponse>, (StatusCode, String)> {
    let host = state
        .monitoring_service
        .collect_all()
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok(Json(DisksResponse {
        timestamp: host.timestamp.to_rfc3339(),
        disks: serde_json::to_value(&host.disks).unwrap(),
    }))
}

/// Handler for GET /api/network
pub async fn network_handler(
    State(state): State<AppState>,
) -> Result<Json<NetworkResponse>, (StatusCode, String)> {
    let host = state
        .monitoring_service
        .collect_all()
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok(Json(NetworkResponse {
        timestamp: host.timestamp.to_rfc3339(),
        interfaces: serde_json::to_value(&host.network_interfaces).unwrap(),
    }))
}

/// Handler for GET /api/dashboard (aggregated endpoint)
pub async fn dashboard_handler(
    State(state): State<AppState>,
) -> Result<Json<DashboardResponse>, (StatusCode, String)> {
    let host = state
        .monitoring_service
        .collect_all()
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let stacks = state
        .monitoring_service
        .get_stacks()
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let processes = state
        .monitoring_service
        .get_top_processes_by_cpu(20)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok(Json(DashboardResponse {
        host: HostResponse::from(&host),
        containers: host.containers.clone(),
        stacks,
        processes,
        disks: serde_json::to_value(&host.disks).unwrap(),
        network: serde_json::to_value(&host.network_interfaces).unwrap(),
    }))
}
