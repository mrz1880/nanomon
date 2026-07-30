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

#[cfg(test)]
mod tests {
    use super::{Temperature, TemperatureSource};

    fn temp(current: f64) -> Temperature {
        Temperature::new("Package id 0".to_string(), TemperatureSource::Cpu, current)
    }

    #[test]
    fn new_has_no_thresholds() {
        let t = temp(50.0);
        assert_eq!(t.label, "Package id 0");
        assert_eq!(t.current_celsius, 50.0);
        assert!(t.high_celsius.is_none());
        assert!(t.critical_celsius.is_none());
    }

    #[test]
    fn without_thresholds_nothing_is_high_or_critical() {
        // A blazing reading still reports false when no threshold is known.
        let t = temp(120.0);
        assert!(!t.is_high());
        assert!(!t.is_critical());
    }

    #[test]
    fn thresholds_are_inclusive() {
        let t = temp(80.0).with_thresholds(Some(80.0), Some(90.0));
        assert!(t.is_high()); // equal to the high threshold counts
        assert!(!t.is_critical());

        let t = temp(90.0).with_thresholds(Some(80.0), Some(90.0));
        assert!(t.is_high());
        assert!(t.is_critical());
    }

    #[test]
    fn below_thresholds_is_neither() {
        let t = temp(79.9).with_thresholds(Some(80.0), Some(90.0));
        assert!(!t.is_high());
        assert!(!t.is_critical());
    }

    #[test]
    fn thresholds_are_independent() {
        // Only a critical threshold set: high stays false.
        let t = temp(95.0).with_thresholds(None, Some(90.0));
        assert!(!t.is_high());
        assert!(t.is_critical());
    }

    #[test]
    fn source_serializes_lowercase() {
        let json = serde_json::to_string(&TemperatureSource::Disk).unwrap();
        assert_eq!(json, "\"disk\"");
    }
}
