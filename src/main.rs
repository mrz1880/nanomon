mod adapters;
mod application;
mod config;
mod domain;
mod interface;
mod ports;

use std::sync::Arc;

use tracing::{info, warn};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use adapters::{DockerAdapter, ProcfsAdapter, ProcfsConfig};
use application::MonitoringService;
use config::Config;
use interface::http::create_router;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // Load configuration
    let config = Config::from_env();

    // Initialize logging
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| format!("nanomon={},tower_http=info", config.log_level).into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    info!("🚀 Starting NanoMon v{}", env!("CARGO_PKG_VERSION"));
    info!("Configuration: {:?}", config);

    // Initialize adapters
    let procfs_config = ProcfsConfig::new(config.proc_path.clone(), config.sys_path.clone());
    let procfs_adapter = ProcfsAdapter::new(procfs_config);

    let docker_adapter = match DockerAdapter::new() {
        Ok(adapter) => {
            info!("✓ Connected to Docker daemon");
            Arc::new(adapter) as Arc<dyn ports::ContainerSource>
        }
        Err(e) => {
            warn!("⚠ Failed to connect to Docker: {}. Container monitoring disabled.", e);
            // Create a no-op adapter (to be implemented) or exit
            return Err(e);
        }
    };

    // Create monitoring service
    let monitoring_service = Arc::new(MonitoringService::new(
        Arc::new(procfs_adapter.system_source()),
        docker_adapter,
        Arc::new(procfs_adapter.process_source()),
    ));

    info!("✓ Monitoring service initialized");

    // Create HTTP server
    let app = create_router(monitoring_service);
    let addr = format!("0.0.0.0:{}", config.port);
    let listener = tokio::net::TcpListener::bind(&addr).await?;

    info!("✓ NanoMon listening on {}", addr);
    info!("  → Dashboard: http://localhost:{}", config.port);
    info!("  → API: http://localhost:{}/api/dashboard", config.port);

    axum::serve(listener, app).await?;

    Ok(())
}
