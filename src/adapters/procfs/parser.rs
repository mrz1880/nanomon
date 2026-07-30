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
        self.user
            + self.nice
            + self.system
            + self.idle
            + self.iowait
            + self.irq
            + self.softirq
            + self.steal
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
        user: parts[0]
            .parse()
            .map_err(|e| ParseError::Parse(format!("user: {}", e)))?,
        nice: parts[1]
            .parse()
            .map_err(|e| ParseError::Parse(format!("nice: {}", e)))?,
        system: parts[2]
            .parse()
            .map_err(|e| ParseError::Parse(format!("system: {}", e)))?,
        idle: parts[3]
            .parse()
            .map_err(|e| ParseError::Parse(format!("idle: {}", e)))?,
        iowait: parts[4]
            .parse()
            .map_err(|e| ParseError::Parse(format!("iowait: {}", e)))?,
        irq: parts[5]
            .parse()
            .map_err(|e| ParseError::Parse(format!("irq: {}", e)))?,
        softirq: parts[6]
            .parse()
            .map_err(|e| ParseError::Parse(format!("softirq: {}", e)))?,
        steal: parts[7]
            .parse()
            .map_err(|e| ParseError::Parse(format!("steal: {}", e)))?,
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

    let start = content
        .find('(')
        .ok_or_else(|| ParseError::Parse("No ( found".to_string()))?;
    let end = content
        .rfind(')')
        .ok_or_else(|| ParseError::Parse("No ) found".to_string()))?;

    let pid_str = content[..start].trim();
    let after_comm = &content[end + 1..];

    let parts: Vec<&str> = after_comm.split_whitespace().collect();
    // Fields read below go up to index 21 (rss); a shorter line must be
    // rejected rather than panic on indexing.
    if parts.len() < 22 {
        return Err(ParseError::Parse("Incomplete proc stat".to_string()));
    }

    let pid: u32 = pid_str
        .parse()
        .map_err(|e| ParseError::Parse(format!("pid: {}", e)))?;
    let state = parts[0].chars().next().unwrap_or('?');
    let ppid: u32 = parts[1]
        .parse()
        .map_err(|e| ParseError::Parse(format!("ppid: {}", e)))?;
    let utime: u64 = parts[11]
        .parse()
        .map_err(|e| ParseError::Parse(format!("utime: {}", e)))?;
    let stime: u64 = parts[12]
        .parse()
        .map_err(|e| ParseError::Parse(format!("stime: {}", e)))?;
    let rss: u64 = parts[21]
        .parse()
        .map_err(|e| ParseError::Parse(format!("rss: {}", e)))?;

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
    use std::io::Write;
    use std::path::PathBuf;

    /// A throwaway directory for filesystem-backed parsers.
    fn scratch(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "nanomon-parser-{}-{}-{label}",
            std::process::id(),
            label
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write(dir: &Path, name: &str, contents: &str) {
        let mut f = fs::File::create(dir.join(name)).unwrap();
        f.write_all(contents.as_bytes()).unwrap();
    }

    /// A realistic /proc/<pid>/stat line: `pid (comm) state ppid ...`, where
    /// index 11/12 after the comm are utime/stime and index 21 is rss.
    fn proc_stat_line(pid: u32, comm: &str, state: char, ppid: u32) -> String {
        let mut fields = vec![state.to_string(), ppid.to_string()];
        // pgrp..cmajflt (indices 2..10)
        for _ in 2..11 {
            fields.push("0".to_string());
        }
        fields.push("150".to_string()); // 11: utime
        fields.push("50".to_string()); // 12: stime
                                       // cutime..vsize (indices 13..20)
        for _ in 13..21 {
            fields.push("0".to_string());
        }
        fields.push("2048".to_string()); // 21: rss (pages)
        format!("{pid} ({comm}) {}", fields.join(" "))
    }

    // --- /proc/uptime ---

    #[test]
    fn uptime_truncates_to_whole_seconds() {
        assert_eq!(parse_uptime("12345.67 98765.43\n").unwrap(), 12345);
        assert_eq!(parse_uptime("0.99").unwrap(), 0);
    }

    #[test]
    fn uptime_rejects_empty_and_garbage() {
        assert!(parse_uptime("").is_err());
        assert!(parse_uptime("   \n").is_err());
        assert!(parse_uptime("not-a-number 1").is_err());
    }

    // --- /proc/loadavg ---

    #[test]
    fn loadavg_reads_the_three_windows() {
        let (one, five, fifteen) = parse_loadavg("0.52 0.78 1.21 2/456 12345\n").unwrap();
        assert_eq!(one, 0.52);
        assert_eq!(five, 0.78);
        assert_eq!(fifteen, 1.21);
    }

    #[test]
    fn loadavg_rejects_short_or_invalid_lines() {
        assert!(parse_loadavg("0.52 0.78").is_err());
        assert!(parse_loadavg("").is_err());
        assert!(parse_loadavg("a 0.78 1.21").is_err());
        assert!(parse_loadavg("0.52 b 1.21").is_err());
        assert!(parse_loadavg("0.52 0.78 c").is_err());
    }

    // --- /proc/stat ---

    #[test]
    fn cpu_stat_reads_every_field() {
        let stat = parse_cpu_stat("cpu  1000 100 500 10000 200 50 30 5\ncpu0 1 2 3\n").unwrap();
        assert_eq!(stat.user, 1000);
        assert_eq!(stat.nice, 100);
        assert_eq!(stat.system, 500);
        assert_eq!(stat.idle, 10000);
        assert_eq!(stat.iowait, 200);
        assert_eq!(stat.irq, 50);
        assert_eq!(stat.softirq, 30);
        assert_eq!(stat.steal, 5);
    }

    #[test]
    fn cpu_stat_totals_exclude_idle_time_from_busy() {
        let stat = parse_cpu_stat("cpu  1000 100 500 10000 200 50 30 5").unwrap();
        assert_eq!(stat.total(), 11885);
        // busy = total - idle - iowait
        assert_eq!(stat.busy(), 11885 - 10000 - 200);
    }

    #[test]
    fn cpu_stat_rejects_malformed_input() {
        assert!(parse_cpu_stat("").is_err());
        // The aggregate line must come first.
        assert!(parse_cpu_stat("cpu0 1 2 3 4 5 6 7 8").is_err());
        assert!(parse_cpu_stat("cpu  1 2 3").is_err());
        assert!(parse_cpu_stat("cpu  a 2 3 4 5 6 7 8").is_err());
    }

    // --- /proc/meminfo ---

    #[test]
    fn meminfo_converts_kilobytes_to_bytes() {
        let content = "MemTotal:       16384 kB\nMemFree:         1024 kB\n";
        let map = parse_meminfo(content).unwrap();
        assert_eq!(map.get("MemTotal"), Some(&(16384 * 1024)));
        assert_eq!(map.get("MemFree"), Some(&(1024 * 1024)));
    }

    #[test]
    fn meminfo_skips_lines_it_cannot_read() {
        let content =
            "MemTotal:       16384 kB\ngarbage\nHugePages_Total:    0\nBad: not-a-number\n";
        let map = parse_meminfo(content).unwrap();
        assert_eq!(map.len(), 2); // MemTotal + HugePages_Total
        assert!(!map.contains_key("Bad"));
        // Values without a kB suffix are taken as-is (still scaled).
        assert_eq!(map.get("HugePages_Total"), Some(&0));
    }

    // --- /proc/mounts ---

    #[test]
    fn mounts_reads_device_point_and_filesystem() {
        let content = "/dev/sda1 / ext4 rw,relatime 0 0\nproc /proc proc rw 0 0\n";
        let mounts = parse_mounts(content).unwrap();
        assert_eq!(mounts.len(), 2);
        assert_eq!(mounts[0].device, "/dev/sda1");
        assert_eq!(mounts[0].mount_point, "/");
        assert_eq!(mounts[0].filesystem, "ext4");
        assert_eq!(mounts[1].filesystem, "proc");
    }

    #[test]
    fn mounts_skips_incomplete_lines() {
        let mounts = parse_mounts("/dev/sda1 /\n\n/dev/sdb /data xfs rw 0 0\n").unwrap();
        assert_eq!(mounts.len(), 1);
        assert_eq!(mounts[0].mount_point, "/data");
    }

    // --- /sys/class/net/<iface>/statistics ---

    #[test]
    fn net_stats_reads_the_four_counters() {
        let dir = scratch("net-ok");
        write(&dir, "rx_bytes", "1000\n");
        write(&dir, "tx_bytes", "2000\n");
        write(&dir, "rx_errors", "1\n");
        write(&dir, "tx_errors", "2\n");

        let (rx, tx, rx_err, tx_err) = parse_net_stats(&dir).unwrap();
        assert_eq!((rx, tx, rx_err, tx_err), (1000, 2000, 1, 2));

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn net_stats_errors_on_missing_or_invalid_counters() {
        let missing = scratch("net-missing");
        assert!(parse_net_stats(&missing).is_err());

        write(&missing, "rx_bytes", "not-a-number\n");
        write(&missing, "tx_bytes", "1");
        write(&missing, "rx_errors", "0");
        write(&missing, "tx_errors", "0");
        assert!(parse_net_stats(&missing).is_err());

        fs::remove_dir_all(&missing).ok();
    }

    // --- /proc/<pid>/stat ---

    #[test]
    fn proc_stat_reads_pid_state_and_times() {
        let line = proc_stat_line(1234, "nginx", 'S', 1);
        let (pid, ppid, state, utime, stime, rss) = parse_proc_stat(&line).unwrap();
        assert_eq!(pid, 1234);
        assert_eq!(ppid, 1);
        assert_eq!(state, 'S');
        assert_eq!(utime, 150);
        assert_eq!(stime, 50);
        assert_eq!(rss, 2048);
    }

    #[test]
    fn proc_stat_handles_comm_with_spaces_and_parens() {
        // The command name is untrusted: it can contain spaces and parentheses,
        // so parsing keys off the last ')'.
        let line = proc_stat_line(7, "my app (worker)", 'R', 2);
        let (pid, ppid, state, ..) = parse_proc_stat(&line).unwrap();
        assert_eq!(pid, 7);
        assert_eq!(ppid, 2);
        assert_eq!(state, 'R');
    }

    #[test]
    fn proc_stat_rejects_truncated_lines_instead_of_panicking() {
        // Fewer fields than the highest index read (rss at 21).
        assert!(parse_proc_stat("1 (x) S 1 0 0 0 0 0 0 0 0 0 100 50").is_err());
        assert!(parse_proc_stat("1234 nginx S 1").is_err()); // no parentheses
        assert!(parse_proc_stat("").is_err());
    }

    #[test]
    fn proc_stat_rejects_non_numeric_ids() {
        let line = proc_stat_line(1, "x", 'S', 1).replacen("1 (x)", "abc (x)", 1);
        assert!(parse_proc_stat(&line).is_err());
    }

    // --- /proc/<pid>/status ---

    #[test]
    fn status_reads_the_real_uid() {
        let content = "Name:\tnginx\nUid:\t1000\t1000\t1000\t1000\nGid:\t100\n";
        assert_eq!(parse_proc_status_uid(content).unwrap(), 1000);
    }

    #[test]
    fn status_errors_when_uid_is_absent_or_unreadable() {
        assert!(parse_proc_status_uid("Name:\tnginx\nGid:\t100\n").is_err());
        assert!(parse_proc_status_uid("").is_err());
        assert!(parse_proc_status_uid("Uid:\troot\n").is_err());
        assert!(parse_proc_status_uid("Uid:\n").is_err());
    }
}
