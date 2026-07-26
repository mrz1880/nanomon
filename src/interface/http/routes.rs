use std::sync::Arc;

use axum::{routing::get, Router};
use tower_http::{cors::CorsLayer, services::ServeDir};

use crate::application::{Collector, MonitoringService};

use super::handlers::{
    container_detail_handler, containers_handler, dashboard_handler, disks_handler, health_handler,
    history_handler, host_handler, live_history_handler, network_handler, processes_handler,
    prometheus_handler, services_handler, stream_handler, AppState,
};

pub fn create_router(
    monitoring_service: Arc<MonitoringService>,
    live: Arc<Collector>,
    sse_keepalive: u64,
) -> Router {
    let state = AppState {
        monitoring_service,
        live,
        sse_keepalive,
    };

    Router::new()
        // API routes
        .route("/api/health", get(health_handler))
        .route("/api/host", get(host_handler))
        .route("/api/containers", get(containers_handler))
        .route("/api/containers/{name}", get(container_detail_handler))
        .route("/api/processes", get(processes_handler))
        .route("/api/disks", get(disks_handler))
        .route("/api/network", get(network_handler))
        .route("/api/dashboard", get(dashboard_handler))
        .route("/api/history", get(history_handler))
        .route("/api/live/history", get(live_history_handler))
        .route("/api/stream", get(stream_handler))
        .route("/api/services", get(services_handler))
        // Prometheus metrics
        .route("/metrics", get(prometheus_handler))
        // Serve static files
        .nest_service("/static", ServeDir::new("src/interface/web/static"))
        .fallback_service(ServeDir::new("src/interface/web/static"))
        .layer(CorsLayer::permissive())
        .with_state(state)
}
