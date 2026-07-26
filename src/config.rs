use std::env;
use std::net::{AddrParseError, SocketAddr};
use std::path::PathBuf;

/// Build a listen address from a bind string and port, validating the IP.
fn build_socket_addr(bind: &str, port: u16) -> Result<SocketAddr, AddrParseError> {
    Ok(SocketAddr::new(bind.parse()?, port))
}

/// Application configuration
#[derive(Debug, Clone)]
pub struct Config {
    pub port: u16,
    pub bind: String,
    pub poll_interval: u64,
    pub fast_interval: u64,
    pub hires_size: usize,
    pub sse_keepalive: u64,
    pub history_size: usize,
    #[allow(dead_code)]
    pub process_limit: usize,
    #[allow(dead_code)]
    pub docker_socket: String,
    pub proc_path: PathBuf,
    pub sys_path: PathBuf,
    pub log_level: String,
    pub enable_systemd: bool,
    pub service_filter: Vec<String>,
    pub watch_dirs: Vec<PathBuf>,
    pub alert_config_path: Option<PathBuf>,
}

impl Config {
    pub fn from_env() -> Self {
        Self {
            port: env::var("NANOMON_PORT")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(3000),
            bind: env::var("NANOMON_BIND").unwrap_or_else(|_| "0.0.0.0".to_string()),
            poll_interval: env::var("NANOMON_POLL_INTERVAL")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(10),
            fast_interval: env::var("NANOMON_FAST_INTERVAL")
                .ok()
                .and_then(|s| s.parse().ok())
                .filter(|&v| v >= 1)
                .unwrap_or(1),
            hires_size: env::var("NANOMON_HIRES_SIZE")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(120),
            sse_keepalive: env::var("NANOMON_SSE_KEEPALIVE")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(15),
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
            service_filter: env::var("NANOMON_SERVICE_FILTER")
                .map(|s| {
                    s.split(',')
                        .map(|p| p.trim().to_string())
                        .filter(|p| !p.is_empty())
                        .collect()
                })
                .unwrap_or_default(),
            watch_dirs: env::var("NANOMON_WATCH_DIRS")
                .map(|s| {
                    s.split(',')
                        .map(|p| p.trim())
                        .filter(|p| !p.is_empty())
                        .map(PathBuf::from)
                        .collect()
                })
                .unwrap_or_default(),
            alert_config_path: env::var("NANOMON_ALERT_CONFIG").ok().map(PathBuf::from),
        }
    }

    /// Resolve the HTTP listen address, validating `NANOMON_BIND` as an IP.
    pub fn socket_addr(&self) -> Result<SocketAddr, AddrParseError> {
        build_socket_addr(&self.bind, self.port)
    }
}

impl Default for Config {
    fn default() -> Self {
        Self::from_env()
    }
}

#[cfg(test)]
mod tests {
    use super::build_socket_addr;

    #[test]
    fn default_bind_is_all_interfaces() {
        let addr = build_socket_addr("0.0.0.0", 3000).unwrap();
        assert_eq!(addr.to_string(), "0.0.0.0:3000");
    }

    #[test]
    fn loopback_bind_is_loopback_only() {
        let addr = build_socket_addr("127.0.0.1", 3000).unwrap();
        assert!(addr.ip().is_loopback());
    }

    #[test]
    fn ipv6_bind_parses() {
        let addr = build_socket_addr("::1", 8080).unwrap();
        assert!(addr.ip().is_loopback());
        assert_eq!(addr.port(), 8080);
    }

    #[test]
    fn invalid_bind_is_rejected() {
        assert!(build_socket_addr("not-an-ip", 3000).is_err());
    }
}
