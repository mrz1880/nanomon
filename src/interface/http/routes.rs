use std::sync::Arc;

use axum::{
    routing::get,
    Router,
};
use tower_http::{cors::CorsLayer, services::ServeDir};

use crate::application::MonitoringService;

use super::handlers::{
    containers_handler, dashboard_handler, disks_handler, health_handler, host_handler,
    network_handler, processes_handler, AppState,
};

pub fn create_router(monitoring_service: Arc<MonitoringService>) -> Router {
    let state = AppState {
        monitoring_service,
    };

    Router::new()
        // API routes
        .route("/api/health", get(health_handler))
        .route("/api/host", get(host_handler))
        .route("/api/containers", get(containers_handler))
        .route("/api/processes", get(processes_handler))
        .route("/api/disks", get(disks_handler))
        .route("/api/network", get(network_handler))
        .route("/api/dashboard", get(dashboard_handler))
        // Serve static files
        .nest_service("/static", ServeDir::new("src/interface/web/static"))
        .nest_service("/", ServeDir::new("src/interface/web/static"))
        .layer(CorsLayer::permissive())
        .with_state(state)
}
