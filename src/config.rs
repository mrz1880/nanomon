use std::env;
use std::path::PathBuf;

/// Application configuration
#[derive(Debug, Clone)]
pub struct Config {
    pub port: u16,
    pub poll_interval: u64,
    pub history_size: usize,
    #[allow(dead_code)]
    pub process_limit: usize,
    #[allow(dead_code)]
    pub docker_socket: String,
    pub proc_path: PathBuf,
    pub sys_path: PathBuf,
    pub log_level: String,
    pub enable_systemd: bool,
    pub alert_config_path: Option<PathBuf>,
}

impl Config {
    pub fn from_env() -> Self {
        Self {
            port: env::var("NANOMON_PORT")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(3000),
            poll_interval: env::var("NANOMON_POLL_INTERVAL")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(10),
            history_size: env::var("NANOMON_HISTORY_SIZE")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(360),
            process_limit: env::var("NANOMON_PROCESS_LIMIT")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(20),
            docker_socket: env::var("DOCKER_HOST")
                .unwrap_or_else(|_| "unix:///var/run/docker.sock".to_string()),
            proc_path: env::var("NANOMON_PROC_PATH")
                .map(PathBuf::from)
                .unwrap_or_else(|_| PathBuf::from("/proc")),
            sys_path: env::var("NANOMON_SYS_PATH")
                .map(PathBuf::from)
                .unwrap_or_else(|_| PathBuf::from("/sys")),
            log_level: env::var("NANOMON_LOG_LEVEL").unwrap_or_else(|_| "info".to_string()),
            enable_systemd: env::var("NANOMON_ENABLE_SYSTEMD")
                .map(|s| s == "true" || s == "1")
                .unwrap_or(false),
            alert_config_path: env::var("NANOMON_ALERT_CONFIG").ok().map(PathBuf::from),
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        Self::from_env()
    }
}
