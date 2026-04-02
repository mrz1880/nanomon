use serde::{Deserialize, Serialize};

/// Temperature reading from a sensor
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Temperature {
    pub label: String,
    pub source: TemperatureSource,
    pub current_celsius: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub high_celsius: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub critical_celsius: Option<f64>,
}

impl Temperature {
    pub fn new(label: String, source: TemperatureSource, current_celsius: f64) -> Self {
        Self {
            label,
            source,
            current_celsius,
            high_celsius: None,
            critical_celsius: None,
        }
    }

    pub fn with_thresholds(mut self, high: Option<f64>, critical: Option<f64>) -> Self {
        self.high_celsius = high;
        self.critical_celsius = critical;
        self
    }

    #[allow(dead_code)]
    pub fn is_critical(&self) -> bool {
        self.critical_celsius
            .map(|c| self.current_celsius >= c)
            .unwrap_or(false)
    }

    #[allow(dead_code)]
    pub fn is_high(&self) -> bool {
        self.high_celsius
            .map(|h| self.current_celsius >= h)
            .unwrap_or(false)
    }
}

/// Source type for a temperature reading
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TemperatureSource {
    Cpu,
    Disk,
    Other,
}
