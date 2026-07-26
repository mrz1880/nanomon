use std::time::Duration;

use async_trait::async_trait;

use crate::domain::ExternalMetric;
use crate::ports::ExternalMetricSource;

/// Cap on metrics kept per target, to bound cardinality / memory.
const MAX_METRICS_PER_TARGET: usize = 200;

/// A scrape target: a logical job name and an HTTP endpoint.
#[derive(Debug, Clone)]
pub struct ScrapeTarget {
    pub job: String,
    pub url: String,
}

/// Adapter that scrapes external HTTP endpoints exposing Prometheus text or a
/// flat JSON object of name -> number.
pub struct HttpScrapeSource {
    targets: Vec<ScrapeTarget>,
    client: reqwest::Client,
}

impl HttpScrapeSource {
    pub fn new(targets: Vec<ScrapeTarget>) -> Self {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(5))
            .build()
            .expect("Failed to build HTTP client");
        Self { targets, client }
    }
}

/// Parse Prometheus text exposition into flat (name, value) metrics. Labels are
/// dropped (minimal ingestion); comments and unparseable lines are skipped.
fn parse_prometheus(job: &str, body: &str, max: usize) -> Vec<ExternalMetric> {
    let mut out = Vec::new();
    for line in body.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        // "name{labels} value" or "name value" -> split off the trailing value.
        let Some((left, value_str)) = line.rsplit_once(char::is_whitespace) else {
            continue;
        };
        let Ok(value) = value_str.trim().parse::<f64>() else {
            continue;
        };
        let name = left.split('{').next().unwrap_or(left).trim().to_string();
        if name.is_empty() {
            continue;
        }
        out.push(ExternalMetric {
            job: job.to_string(),
            name,
            value,
        });
        if out.len() >= max {
            break;
        }
    }
    out
}

/// Parse a flat JSON object `{ "name": number, ... }` into metrics.
fn parse_json_flat(job: &str, body: &str, max: usize) -> Vec<ExternalMetric> {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(body) else {
        return Vec::new();
    };
    let Some(obj) = value.as_object() else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for (key, val) in obj {
        if let Some(number) = val.as_f64() {
            out.push(ExternalMetric {
                job: job.to_string(),
                name: key.clone(),
                value: number,
            });
            if out.len() >= max {
                break;
            }
        }
    }
    out
}

/// Pick the parser by content shape: JSON if it starts with `{`, else Prometheus.
fn parse_body(job: &str, body: &str, max: usize) -> Vec<ExternalMetric> {
    if body.trim_start().starts_with('{') {
        parse_json_flat(job, body, max)
    } else {
        parse_prometheus(job, body, max)
    }
}

#[async_trait]
impl ExternalMetricSource for HttpScrapeSource {
    async fn scrape(&self) -> Vec<ExternalMetric> {
        let mut all = Vec::new();
        for target in &self.targets {
            match self.client.get(&target.url).send().await {
                Ok(resp) => match resp.text().await {
                    Ok(body) => {
                        all.extend(parse_body(&target.job, &body, MAX_METRICS_PER_TARGET));
                    }
                    Err(e) => tracing::warn!("Scrape body error for {}: {}", target.job, e),
                },
                Err(e) => {
                    tracing::warn!("Scrape failed for {} ({}): {}", target.job, target.url, e)
                }
            }
        }
        all
    }
}

#[cfg(test)]
mod tests {
    use super::{parse_body, parse_json_flat, parse_prometheus};

    #[test]
    fn parses_prometheus_text() {
        let body = "# HELP foo bar\n# TYPE foo gauge\nfoo 1.5\nbar{a=\"b\"} 42\ngarbage line\n";
        let metrics = parse_prometheus("job", body, 100);
        assert_eq!(metrics.len(), 2);
        assert_eq!(metrics[0].name, "foo");
        assert_eq!(metrics[0].value, 1.5);
        assert_eq!(metrics[1].name, "bar"); // labels stripped
        assert_eq!(metrics[1].value, 42.0);
        assert_eq!(metrics[0].job, "job");
    }

    #[test]
    fn comment_lines_are_never_metrics() {
        // A comment whose last token is a number must still be skipped, not
        // parsed as a metric (guards the empty-or-comment skip condition).
        let metrics = parse_prometheus("j", "# threshold is 100\nreal 5", 100);
        assert_eq!(metrics.len(), 1);
        assert_eq!(metrics[0].name, "real");
        assert_eq!(metrics[0].value, 5.0);
    }

    #[test]
    fn parses_flat_json() {
        let body = r#"{"temp": 55, "load": 0.7, "name": "ignored"}"#;
        let metrics = parse_json_flat("j", body, 100);
        assert_eq!(metrics.len(), 2); // the string value is ignored
        assert!(metrics.iter().any(|m| m.name == "temp" && m.value == 55.0));
        assert!(metrics.iter().any(|m| m.name == "load" && m.value == 0.7));
    }

    #[test]
    fn cardinality_is_capped() {
        let body: String = (0..500).map(|i| format!("m{i} {i}\n")).collect();
        assert_eq!(parse_prometheus("j", &body, 10).len(), 10);
    }

    #[test]
    fn parse_body_selects_by_shape() {
        assert_eq!(parse_body("j", "  {\"a\": 1}", 100).len(), 1);
        assert_eq!(parse_body("j", "a 1\nb 2", 100).len(), 2);
    }
}
