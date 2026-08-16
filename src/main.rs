mod adapters;
mod application;
mod config;
mod domain;
mod interface;
mod ports;

use std::sync::Arc;

use tracing::{info, warn};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use adapters::{
    DockerAdapter, FsDirectoryUsage, HttpScrapeSource, MemoryStore, NoContainers, ProcfsAdapter,
    ProcfsConfig, ScrapeTarget, SystemctlAdapter, WebhookSink,
};
use application::{AlertEvaluator, Collector, DirectoryUsageService, MonitoringService};
use config::Config;
use domain::{AlertRule, ServiceAlertRule};
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

    let (docker_adapter, docker_available) = match DockerAdapter::new() {
        Ok(adapter) => {
            info!("Connected to Docker daemon");
            (Arc::new(adapter) as Arc<dyn ports::ContainerSource>, true)
        }
        Err(e) => {
            // Not fatal: a seedbox or a VPS has no Docker daemon and never
            // will. Refusing to start there would cost the operator CPU,
            // memory, disks, network and systemd services over the one source
            // that happens to be absent — and the log line already said
            // container monitoring was disabled, so failing anyway contradicted
            // it.
            warn!(
                "Failed to connect to Docker: {}. Container monitoring disabled.",
                e
            );
            (
                Arc::new(NoContainers) as Arc<dyn ports::ContainerSource>,
                false,
            )
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
        let systemd_adapter = Arc::new(SystemctlAdapter::new(config.service_filter.clone()));
        monitoring_service = monitoring_service.with_service_source(systemd_adapter);
    }

    let monitoring_service = Arc::new(monitoring_service);

    info!("Monitoring service initialized");

    // Startup self-check: report which sources are available so operators can
    // see, at a glance, what a reduced-privilege deployment can and cannot read.
    let procfs_ok = std::fs::metadata(config.proc_path.join("stat")).is_ok();
    info!("Source self-check:");
    info!(
        "  docker: {}",
        if docker_available {
            "connected"
        } else {
            "unavailable (container monitoring disabled)"
        }
    );
    info!(
        "  procfs ({}): {}",
        config.proc_path.display(),
        if procfs_ok {
            "readable"
        } else {
            "NOT readable"
        }
    );
    info!(
        "  systemd services: {}",
        if monitoring_service.has_services() {
            "available"
        } else {
            "disabled / unavailable"
        }
    );

    // Load alert rules if configured
    let alert_evaluator = load_alert_evaluator(&config);
    if let Some(ref evaluator) = alert_evaluator {
        if evaluator.has_rules() {
            info!("Alert rules loaded");
        }
    }

    // High-resolution store for the live (fast-tier) charts.
    let hires_store = Arc::new(MemoryStore::new(config.hires_size));

    // Optional external metric scraping (Prometheus text / JSON endpoints).
    let external_source: Option<Arc<dyn ports::ExternalMetricSource>> =
        if config.scrape_targets.is_empty() {
            None
        } else {
            let targets: Vec<ScrapeTarget> = config
                .scrape_targets
                .iter()
                .map(|(job, url)| ScrapeTarget {
                    job: job.clone(),
                    url: url.clone(),
                })
                .collect();
            info!("Scraping {} external target(s)", targets.len());
            Some(Arc::new(HttpScrapeSource::new(targets)))
        };

    // Two-speed background collector: fast host loop + slow resource loop.
    let collector = Collector::new(
        monitoring_service.clone(),
        hires_store,
        alert_evaluator,
        external_source,
        config.fast_interval,
        config.poll_interval,
    );
    collector.clone().spawn();

    info!(
        "Background collection started (fast: {}s, slow: {}s)",
        config.fast_interval, config.poll_interval
    );

    // Optional per-directory disk usage, refreshed on the slow interval so the
    // filesystem walk never runs on an HTTP request.
    let dir_usage = if config.watch_dirs.is_empty() {
        Arc::new(DirectoryUsageService::new(None))
    } else {
        let source = Arc::new(FsDirectoryUsage::new(config.watch_dirs.clone()));
        let service = Arc::new(DirectoryUsageService::new(Some(source)));
        info!(
            "Watching {} director(ies) for usage",
            config.watch_dirs.len()
        );

        let refresh = service.clone();
        let interval = config.poll_interval;
        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(std::time::Duration::from_secs(interval));
            loop {
                ticker.tick().await;
                refresh.refresh().await;
            }
        });
        service
    };

    // Create HTTP server
    let app = create_router(
        monitoring_service,
        collector,
        dir_usage,
        config.sse_keepalive,
    );
    let addr = match config.socket_addr() {
        Ok(addr) => addr,
        Err(e) => {
            tracing::error!("Invalid NANOMON_BIND '{}': {}", config.bind, e);
            return Err(e.into());
        }
    };
    let listener = tokio::net::TcpListener::bind(addr).await?;

    info!("NanoMon listening on {}", addr);
    info!("  Dashboard: http://localhost:{}", config.port);
    info!("  API: http://localhost:{}/api/dashboard", config.port);
    info!("  Prometheus: http://localhost:{}/metrics", config.port);

    axum::serve(listener, app).await?;

    Ok(())
}

fn load_alert_evaluator(config: &Config) -> Option<Arc<AlertEvaluator>> {
    let path = config.alert_config_path.as_ref()?;

    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(e) => {
            warn!("Failed to read alert config at {:?}: {}", path, e);
            return None;
        }
    };

    let parsed: AlertConfig = match toml::from_str(&content) {
        Ok(config) => config,
        Err(e) => {
            warn!("Failed to parse alert config: {}", e);
            return None;
        }
    };

    let AlertConfig {
        rules,
        service_rules,
    } = parsed;

    if rules.is_empty() && service_rules.is_empty() {
        return None;
    }

    if !service_rules.is_empty() && !config.enable_systemd {
        warn!(
            "Service alert rules are configured but systemd is disabled \
             (set NANOMON_ENABLE_SYSTEMD=true); they will not fire"
        );
    }

    info!(
        "Loaded {} metric rule(s) and {} service rule(s) from {:?}",
        rules.len(),
        service_rules.len(),
        path
    );
    let sink = Arc::new(WebhookSink::new());
    Some(Arc::new(AlertEvaluator::new(rules, service_rules, sink)))
}

#[derive(serde::Deserialize)]
struct AlertConfig {
    #[serde(default)]
    rules: Vec<AlertRule>,
    #[serde(default)]
    service_rules: Vec<ServiceAlertRule>,
}
