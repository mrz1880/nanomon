use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::domain::{AlertRule, ServiceAlertRule};

/// Rules declared in the alert configuration file.
#[derive(Debug, Default, Deserialize)]
pub struct AlertRules {
    /// Threshold rules on host and external metrics.
    #[serde(default, rename = "rules")]
    pub metric: Vec<AlertRule>,
    /// Rules on systemd unit states.
    #[serde(default, rename = "service_rules")]
    pub service: Vec<ServiceAlertRule>,
}

/// Why the alert configuration could not be loaded.
#[derive(Debug, thiserror::Error)]
pub enum AlertConfigError {
    /// The file is missing or the process may not read it.
    #[error("cannot read alert config {path:?}: {source}")]
    Unreadable {
        path: PathBuf,
        source: std::io::Error,
    },
    /// The file is not valid TOML or does not describe alert rules.
    #[error("invalid alert config {path:?}: {source}")]
    Invalid {
        path: PathBuf,
        source: toml::de::Error,
    },
}

/// Loads the alert rules from a TOML file.
///
/// # Arguments
/// * `path` - Location of the alert configuration file.
///
/// # Returns
/// The metric and service rules it declares, possibly none.
///
/// # Errors
/// Returns [`AlertConfigError`] when the file cannot be read or parsed.
pub fn load_alert_rules(path: &Path) -> Result<AlertRules, AlertConfigError> {
    // `map_err` builds the error only on failure; `to_path_buf` copies the
    // borrowed path so the error owns it and outlives this call.
    let content = std::fs::read_to_string(path).map_err(|source| AlertConfigError::Unreadable {
        path: path.to_path_buf(),
        source,
    })?;
    toml::from_str(&content).map_err(|source| AlertConfigError::Invalid {
        path: path.to_path_buf(),
        source,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Alert config file under the temp dir, removed when dropped.
    struct TempConfig {
        path: PathBuf,
    }

    impl TempConfig {
        fn new(label: &str, contents: &str) -> Self {
            let path = std::env::temp_dir().join(format!(
                "nanomon-alertconfig-{}-{label}.toml",
                std::process::id()
            ));
            std::fs::write(&path, contents).expect("temp dir is writable");
            Self { path }
        }
    }

    impl Drop for TempConfig {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.path);
        }
    }

    #[test]
    fn metric_and_service_rules_are_read_from_their_tables() {
        let config = TempConfig::new(
            "valid",
            r#"
[[rules]]
name = "disk"
metric = { disk_usage = { mount_point = "/" } }
condition = "above"
threshold = 85.0
webhook_url = "https://ntfy.sh/example"

[[service_rules]]
name = "nginx down"
unit = "nginx.service"
alert_when = ["failed"]
webhook_url = "https://ntfy.sh/example"
"#,
        );

        let rules = load_alert_rules(&config.path).expect("valid config");

        assert_eq!(rules.metric.len(), 1);
        assert_eq!(rules.metric[0].name, "disk");
        assert_eq!(rules.service.len(), 1);
    }

    #[test]
    fn a_malformed_file_is_an_error_not_an_empty_rule_set() {
        let config = TempConfig::new("malformed", "[[rules]]\nname = \"disk\"\nthreshold = \n");

        let result = load_alert_rules(&config.path);

        assert!(matches!(result, Err(AlertConfigError::Invalid { .. })));
    }

    #[test]
    fn a_missing_file_is_an_error_not_an_empty_rule_set() {
        let path = std::env::temp_dir().join("nanomon-alertconfig-does-not-exist.toml");

        let result = load_alert_rules(&path);

        assert!(matches!(result, Err(AlertConfigError::Unreadable { .. })));
    }
}
