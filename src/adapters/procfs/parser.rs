use std::collections::HashMap;
use std::fs;
use std::path::Path;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum ParseError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Parse error: {0}")]
    Parse(String),

    #[error("Missing field: {0}")]
    MissingField(String),
}

pub type ParseResult<T> = Result<T, ParseError>;

/// Parse /proc/uptime
pub fn parse_uptime(content: &str) -> ParseResult<u64> {
    let parts: Vec<&str> = content.split_whitespace().collect();
    if parts.is_empty() {
        return Err(ParseError::Parse("Empty uptime file".to_string()));
    }

    let uptime_secs = parts[0]
        .parse::<f64>()
        .map_err(|e| ParseError::Parse(format!("Invalid uptime value: {}", e)))?;

    Ok(uptime_secs as u64)
}

/// Parse /proc/loadavg
pub fn parse_loadavg(content: &str) -> ParseResult<(f64, f64, f64)> {
    let parts: Vec<&str> = content.split_whitespace().collect();
    if parts.len() < 3 {
        return Err(ParseError::Parse("Invalid loadavg format".to_string()));
    }

    let one = parts[0]
        .parse::<f64>()
        .map_err(|e| ParseError::Parse(format!("Invalid load 1min: {}", e)))?;
    let five = parts[1]
        .parse::<f64>()
        .map_err(|e| ParseError::Parse(format!("Invalid load 5min: {}", e)))?;
    let fifteen = parts[2]
        .parse::<f64>()
        .map_err(|e| ParseError::Parse(format!("Invalid load 15min: {}", e)))?;

    Ok((one, five, fifteen))
}

/// CPU stats from /proc/stat
#[derive(Debug, Clone, Default)]
pub struct CpuStat {
    pub user: u64,
    pub nice: u64,
    pub system: u64,
    pub idle: u64,
    pub iowait: u64,
    pub irq: u64,
    pub softirq: u64,
    pub steal: u64,
}

impl CpuStat {
    pub fn total(&self) -> u64 {
        self.user + self.nice + self.system + self.idle + self.iowait + self.irq + self.softirq + self.steal
    }

    pub fn busy(&self) -> u64 {
        self.total() - self.idle - self.iowait
    }
}

/// Parse /proc/stat (first line only for aggregate CPU)
pub fn parse_cpu_stat(content: &str) -> ParseResult<CpuStat> {
    let first_line = content
        .lines()
        .next()
        .ok_or_else(|| ParseError::Parse("Empty stat file".to_string()))?;

    if !first_line.starts_with("cpu ") {
        return Err(ParseError::Parse("Missing cpu line".to_string()));
    }

    let parts: Vec<&str> = first_line.split_whitespace().skip(1).collect();
    if parts.len() < 8 {
        return Err(ParseError::Parse("Incomplete cpu stat".to_string()));
    }

    Ok(CpuStat {
        user: parts[0].parse().map_err(|e| ParseError::Parse(format!("user: {}", e)))?,
        nice: parts[1].parse().map_err(|e| ParseError::Parse(format!("nice: {}", e)))?,
        system: parts[2].parse().map_err(|e| ParseError::Parse(format!("system: {}", e)))?,
        idle: parts[3].parse().map_err(|e| ParseError::Parse(format!("idle: {}", e)))?,
        iowait: parts[4].parse().map_err(|e| ParseError::Parse(format!("iowait: {}", e)))?,
        irq: parts[5].parse().map_err(|e| ParseError::Parse(format!("irq: {}", e)))?,
        softirq: parts[6].parse().map_err(|e| ParseError::Parse(format!("softirq: {}", e)))?,
        steal: parts[7].parse().map_err(|e| ParseError::Parse(format!("steal: {}", e)))?,
    })
}

/// Parse /proc/meminfo into a map
pub fn parse_meminfo(content: &str) -> ParseResult<HashMap<String, u64>> {
    let mut map = HashMap::new();

    for line in content.lines() {
        let parts: Vec<&str> = line.split(':').collect();
        if parts.len() != 2 {
            continue;
        }

        let key = parts[0].trim().to_string();
        let value_str = parts[1].trim().trim_end_matches(" kB");

        if let Ok(value) = value_str.parse::<u64>() {
            map.insert(key, value * 1024); // Convert kB to bytes
        }
    }

    Ok(map)
}

/// Parse /proc/mounts
#[derive(Debug, Clone)]
pub struct MountInfo {
    pub device: String,
    pub mount_point: String,
    pub filesystem: String,
}

pub fn parse_mounts(content: &str) -> ParseResult<Vec<MountInfo>> {
    let mut mounts = Vec::new();

    for line in content.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 3 {
            continue;
        }

        mounts.push(MountInfo {
            device: parts[0].to_string(),
            mount_point: parts[1].to_string(),
            filesystem: parts[2].to_string(),
        });
    }

    Ok(mounts)
}

/// Parse network statistics from /sys/class/net/{interface}/statistics
pub fn parse_net_stats(stats_dir: &Path) -> ParseResult<(u64, u64, u64, u64)> {
    let rx_bytes = fs::read_to_string(stats_dir.join("rx_bytes"))?
        .trim()
        .parse::<u64>()
        .map_err(|e| ParseError::Parse(format!("rx_bytes: {}", e)))?;

    let tx_bytes = fs::read_to_string(stats_dir.join("tx_bytes"))?
        .trim()
        .parse::<u64>()
        .map_err(|e| ParseError::Parse(format!("tx_bytes: {}", e)))?;

    let rx_errors = fs::read_to_string(stats_dir.join("rx_errors"))?
        .trim()
        .parse::<u64>()
        .map_err(|e| ParseError::Parse(format!("rx_errors: {}", e)))?;

    let tx_errors = fs::read_to_string(stats_dir.join("tx_errors"))?
        .trim()
        .parse::<u64>()
        .map_err(|e| ParseError::Parse(format!("tx_errors: {}", e)))?;

    Ok((rx_bytes, tx_bytes, rx_errors, tx_errors))
}

/// Parse /proc/{pid}/stat
pub fn parse_proc_stat(content: &str) -> ParseResult<(u32, u32, char, u64, u64, u64)> {
    // Format: pid (comm) state ppid ... utime stime ...
    // Need to handle comm with spaces and parentheses

    let start = content.find('(').ok_or_else(|| ParseError::Parse("No ( found".to_string()))?;
    let end = content.rfind(')').ok_or_else(|| ParseError::Parse("No ) found".to_string()))?;

    let pid_str = content[..start].trim();
    let after_comm = &content[end + 1..];

    let parts: Vec<&str> = after_comm.split_whitespace().collect();
    if parts.len() < 13 {
        return Err(ParseError::Parse("Incomplete proc stat".to_string()));
    }

    let pid: u32 = pid_str.parse().map_err(|e| ParseError::Parse(format!("pid: {}", e)))?;
    let state = parts[0].chars().next().unwrap_or('?');
    let ppid: u32 = parts[1].parse().map_err(|e| ParseError::Parse(format!("ppid: {}", e)))?;
    let utime: u64 = parts[11].parse().map_err(|e| ParseError::Parse(format!("utime: {}", e)))?;
    let stime: u64 = parts[12].parse().map_err(|e| ParseError::Parse(format!("stime: {}", e)))?;
    let rss: u64 = parts[21].parse().map_err(|e| ParseError::Parse(format!("rss: {}", e)))?;

    Ok((pid, ppid, state, utime, stime, rss))
}

/// Parse /proc/{pid}/status for UID
pub fn parse_proc_status_uid(content: &str) -> ParseResult<u32> {
    for line in content.lines() {
        if line.starts_with("Uid:") {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 2 {
                return parts[1]
                    .parse()
                    .map_err(|e| ParseError::Parse(format!("uid: {}", e)));
            }
        }
    }
    Err(ParseError::MissingField("Uid".to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_uptime() {
        let content = "12345.67 98765.43\n";
        let uptime = parse_uptime(content).unwrap();
        assert_eq!(uptime, 12345);
    }

    #[test]
    fn test_parse_loadavg() {
        let content = "0.52 0.78 1.21 2/456 12345\n";
        let (one, five, fifteen) = parse_loadavg(content).unwrap();
        assert_eq!(one, 0.52);
        assert_eq!(five, 0.78);
        assert_eq!(fifteen, 1.21);
    }

    #[test]
    fn test_parse_cpu_stat() {
        let content = "cpu  1000 100 500 10000 200 50 30 0\n";
        let stat = parse_cpu_stat(content).unwrap();
        assert_eq!(stat.user, 1000);
        assert_eq!(stat.system, 500);
        assert_eq!(stat.idle, 10000);
        assert_eq!(stat.iowait, 200);
    }
}
