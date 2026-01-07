use std::fs;

use async_trait::async_trait;

use crate::domain::{Process, ProcessState};
use crate::ports::ProcessSource;

use super::parser;
use super::ProcfsConfig;

/// Process source implementation using procfs
pub struct ProcfsProcessSource {
    config: ProcfsConfig,
}

impl ProcfsProcessSource {
    pub fn new(config: ProcfsConfig) -> Self {
        Self { config }
    }

    fn list_pids(&self) -> Result<Vec<u32>, Box<dyn std::error::Error + Send + Sync>> {
        let mut pids = Vec::new();

        for entry in fs::read_dir(&self.config.proc_path)? {
            let entry = entry?;
            let file_name = entry.file_name();
            let name = file_name.to_string_lossy();

            if let Ok(pid) = name.parse::<u32>() {
                pids.push(pid);
            }
        }

        Ok(pids)
    }

    fn read_process(&self, pid: u32) -> Result<Process, Box<dyn std::error::Error + Send + Sync>> {
        let pid_path = self.config.proc_path.join(pid.to_string());

        // Read /proc/{pid}/stat
        let stat_content = fs::read_to_string(pid_path.join("stat"))?;
        let (_pid, ppid, state_char, utime, stime, rss) = parser::parse_proc_stat(&stat_content)?;

        // Read /proc/{pid}/status for UID
        let status_content = fs::read_to_string(pid_path.join("status"))?;
        let uid = parser::parse_proc_status_uid(&status_content)?;

        // Get username from UID (simple approach)
        let user = self.get_username_from_uid(uid).unwrap_or_else(|| uid.to_string());

        // Read command from /proc/{pid}/cmdline
        let cmdline_content = fs::read_to_string(pid_path.join("cmdline")).unwrap_or_default();
        let command = if cmdline_content.is_empty() {
            // Kernel thread, use comm
            fs::read_to_string(pid_path.join("comm"))
                .unwrap_or_else(|_| format!("[pid:{}]", pid))
                .trim()
                .to_string()
        } else {
            // Replace null bytes with spaces and take first arg
            cmdline_content.replace('\0', " ").trim().to_string()
        };

        // Read system uptime and calculate CPU usage (simplified, needs delta)
        let uptime_content = fs::read_to_string(self.config.proc_path.join("uptime"))?;
        let uptime = parser::parse_uptime(&uptime_content)?;
        let hertz = 100; // Typical USER_HZ value
        let total_time = utime + stime;
        let seconds = uptime - (total_time / hertz);
        let cpu_percent = if seconds > 0 {
            (total_time as f64 / hertz as f64 / seconds as f64) * 100.0
        } else {
            0.0
        };

        // Memory usage (RSS in pages, typically 4096 bytes)
        let page_size = 4096;
        let memory_bytes = rss * page_size;

        // Get total memory for percentage
        let meminfo_content = fs::read_to_string(self.config.proc_path.join("meminfo"))?;
        let meminfo = parser::parse_meminfo(&meminfo_content)?;
        let total_memory = *meminfo.get("MemTotal").unwrap_or(&1);
        let memory_percent = (memory_bytes as f64 / total_memory as f64) * 100.0;

        // Check if process is in a container by examining cgroup
        let container_id = self.get_container_id_from_cgroup(pid)?;

        Ok(Process::new(
            pid,
            ppid,
            user,
            command,
            ProcessState::from_char(state_char),
        )
        .with_metrics(cpu_percent, memory_percent, memory_bytes)
        .with_container(container_id))
    }

    fn get_username_from_uid(&self, uid: u32) -> Option<String> {
        // Simple implementation: read /etc/passwd
        let passwd = fs::read_to_string("/etc/passwd").ok()?;
        for line in passwd.lines() {
            let parts: Vec<&str> = line.split(':').collect();
            if parts.len() >= 3 {
                if let Ok(line_uid) = parts[2].parse::<u32>() {
                    if line_uid == uid {
                        return Some(parts[0].to_string());
                    }
                }
            }
        }
        None
    }

    fn get_container_id_from_cgroup(&self, pid: u32) -> Result<Option<crate::domain::ContainerId>, Box<dyn std::error::Error + Send + Sync>> {
        let cgroup_path = self.config.proc_path.join(format!("{}/cgroup", pid));
        let content = fs::read_to_string(cgroup_path).unwrap_or_default();

        // Look for docker container ID in cgroup path
        // Format: 0::/docker/{container_id}
        for line in content.lines() {
            if line.contains("/docker/") {
                if let Some(id) = line.split("/docker/").nth(1) {
                    let container_id = id.trim_end_matches(".scope").to_string();
                    if !container_id.is_empty() {
                        return Ok(Some(container_id.into()));
                    }
                }
            }
        }

        Ok(None)
    }
}

#[async_trait]
impl ProcessSource for ProcfsProcessSource {
    async fn list_processes(&self) -> Result<Vec<Process>, Box<dyn std::error::Error + Send + Sync>> {
        let pids = self.list_pids()?;
        let mut processes = Vec::new();

        for pid in pids {
            if let Ok(process) = self.read_process(pid) {
                processes.push(process);
            }
        }

        Ok(processes)
    }

    async fn get_top_by_cpu(&self, n: usize) -> Result<Vec<Process>, Box<dyn std::error::Error + Send + Sync>> {
        let mut processes = self.list_processes().await?;
        processes.sort_by(|a, b| b.cpu_percent.partial_cmp(&a.cpu_percent).unwrap());
        processes.truncate(n);
        Ok(processes)
    }

    async fn get_top_by_memory(&self, n: usize) -> Result<Vec<Process>, Box<dyn std::error::Error + Send + Sync>> {
        let mut processes = self.list_processes().await?;
        processes.sort_by(|a, b| b.memory_bytes.cmp(&a.memory_bytes));
        processes.truncate(n);
        Ok(processes)
    }
}
