mod adapters;
mod application;
mod config;
mod domain;
mod interface;
mod ports;

use std::sync::Arc;
use std::time::Duration;

use tracing::{info, warn};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use adapters::{
    DockerAdapter, MemoryStore, ProcfsAdapter, ProcfsConfig, SystemctlAdapter, WebhookSink,
};
use application::{AlertEvaluator, MonitoringService};
use config::Config;
use domain::AlertRule;
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

    info!("Starting NanoMon v{}", env!("CARGO_PKG_VERSION"));
    info!("Configuration: {:?}", config);

    // Initialize adapters
    let procfs_config = ProcfsConfig::new(config.proc_path.clone(), config.sys_path.clone());
    let procfs_adapter = ProcfsAdapter::new(procfs_config);

    let docker_adapter = match DockerAdapter::new() {
        Ok(adapter) => {
            info!("Connected to Docker daemon");
            Arc::new(adapter) as Arc<dyn ports::ContainerSource>
        }
        Err(e) => {
            warn!(
                "Failed to connect to Docker: {}. Container monitoring disabled.",
                e
            );
            return Err(e);
        }
    };

    // Initialize metric store
    let metric_store = Arc::new(MemoryStore::new(config.history_size));

    // Create monitoring service
    let mut monitoring_service = MonitoringService::new(
        Arc::new(procfs_adapter.system_source()),
        docker_adapter,
        Arc::new(procfs_adapter.process_source()),
        metric_store,
    );

    // Optionally enable systemd monitoring
    if config.enable_systemd {
        let systemd_adapter = Arc::new(SystemctlAdapter::new());
        monitoring_service = monitoring_service.with_service_source(systemd_adapter);
    }

    let monitoring_service = Arc::new(monitoring_service);

    info!("Monitoring service initialized");

    // Load alert rules if configured
    let alert_evaluator = load_alert_evaluator(&config);
    if let Some(ref evaluator) = alert_evaluator {
        if evaluator.has_rules() {
            info!("Alert rules loaded");
        }
    }

    // Start background polling loop
    let poll_service = monitoring_service.clone();
    let poll_interval = config.poll_interval;
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(poll_interval));
        loop {
            interval.tick().await;
            match poll_service.collect_all().await {
                Ok(snapshot) => {
                    // Evaluate alerts before storing
                    if let Some(ref evaluator) = alert_evaluator {
                        evaluator.evaluate(&snapshot).await;
                    }
                    poll_service.store_snapshot(snapshot);
                }
                Err(e) => {
                    tracing::error!("Failed to collect metrics: {}", e);
                }
            }
        }
    });

    info!("Background polling started (interval: {}s)", poll_interval);

    // Create HTTP server
    let app = create_router(monitoring_service);
    let addr = format!("0.0.0.0:{}", config.port);
    let listener = tokio::net::TcpListener::bind(&addr).await?;

    info!("NanoMon listening on {}", addr);
    info!("  Dashboard: http://localhost:{}", config.port);
    info!("  API: http://localhost:{}/api/dashboard", config.port);
    info!("  Prometheus: http://localhost:{}/metrics", config.port);

    axum::serve(listener, app).await?;

    Ok(())
}

fn load_alert_evaluator(config: &Config) -> Option<AlertEvaluator> {
    let path = config.alert_config_path.as_ref()?;

    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(e) => {
            warn!("Failed to read alert config at {:?}: {}", path, e);
            return None;
        }
    };

    let rules: Vec<AlertRule> = match toml::from_str(&content) {
        Ok(config) => {
            let parsed: AlertConfig = config;
            parsed.rules
        }
        Err(e) => {
            warn!("Failed to parse alert config: {}", e);
            return None;
        }
    };

    if rules.is_empty() {
        return None;
    }

    info!("Loaded {} alert rules from {:?}", rules.len(), path);
    let sink = Arc::new(WebhookSink::new());
    Some(AlertEvaluator::new(rules, sink))
}

#[derive(serde::Deserialize)]
struct AlertConfig {
    #[serde(default)]
    rules: Vec<AlertRule>,
}
