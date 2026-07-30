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
        let user = self
            .get_username_from_uid(uid)
            .unwrap_or_else(|| uid.to_string());

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

    fn get_container_id_from_cgroup(
        &self,
        pid: u32,
    ) -> Result<Option<crate::domain::ContainerId>, Box<dyn std::error::Error + Send + Sync>> {
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
    async fn list_processes(
        &self,
    ) -> Result<Vec<Process>, Box<dyn std::error::Error + Send + Sync>> {
        let pids = self.list_pids()?;
        let mut processes = Vec::new();

        for pid in pids {
            if let Ok(process) = self.read_process(pid) {
                processes.push(process);
            }
        }

        Ok(processes)
    }

    async fn get_top_by_cpu(
        &self,
        n: usize,
    ) -> Result<Vec<Process>, Box<dyn std::error::Error + Send + Sync>> {
        let mut processes = self.list_processes().await?;
        processes.sort_by(|a, b| {
            b.cpu_percent
                .partial_cmp(&a.cpu_percent)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        processes.truncate(n);
        Ok(processes)
    }

    async fn get_top_by_memory(
        &self,
        n: usize,
    ) -> Result<Vec<Process>, Box<dyn std::error::Error + Send + Sync>> {
        let mut processes = self.list_processes().await?;
        processes.sort_by_key(|p| std::cmp::Reverse(p.memory_bytes));
        processes.truncate(n);
        Ok(processes)
    }
}

#[cfg(test)]
mod tests {
    use super::{ProcfsConfig, ProcfsProcessSource};
    use crate::domain::ProcessState;
    use crate::ports::ProcessSource;
    use std::fs;
    use std::io::Write;
    use std::path::{Path, PathBuf};

    /// Builds a throwaway fake /proc tree.
    struct FakeProc {
        root: PathBuf,
    }

    impl FakeProc {
        fn new(label: &str) -> Self {
            let root =
                std::env::temp_dir().join(format!("nanomon-proc-{}-{label}", std::process::id()));
            let _ = fs::remove_dir_all(&root);
            fs::create_dir_all(&root).unwrap();
            let fake = Self { root };
            // Values every process read depends on.
            fake.write("uptime", "10000.00 40000.00\n");
            fake.write("meminfo", "MemTotal:       1024 kB\n");
            fake
        }

        fn write(&self, name: &str, contents: &str) {
            let path = self.root.join(name);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).unwrap();
            }
            let mut f = fs::File::create(path).unwrap();
            f.write_all(contents.as_bytes()).unwrap();
        }

        /// Adds a process with utime=150, stime=50 and rss=2 pages (8 KiB).
        fn add_process(&self, pid: u32, comm: &str, state: char, cgroup: Option<&str>) {
            let mut fields = vec![state.to_string(), "1".to_string()];
            for _ in 2..11 {
                fields.push("0".to_string());
            }
            fields.push("150".to_string()); // utime
            fields.push("50".to_string()); // stime
            for _ in 13..21 {
                fields.push("0".to_string());
            }
            fields.push("2".to_string()); // rss in pages
            self.write(
                &format!("{pid}/stat"),
                &format!("{pid} ({comm}) {}", fields.join(" ")),
            );
            self.write(&format!("{pid}/status"), "Name:\tx\nUid:\t0\t0\t0\t0\n");
            self.write(&format!("{pid}/cmdline"), &format!("{comm}\0--flag\0"));
            self.write(&format!("{pid}/comm"), &format!("{comm}\n"));
            if let Some(cgroup) = cgroup {
                self.write(&format!("{pid}/cgroup"), cgroup);
            }
        }

        fn source(&self) -> ProcfsProcessSource {
            ProcfsProcessSource::new(ProcfsConfig::new(self.root.clone(), Path::new("/sys")))
        }
    }

    impl Drop for FakeProc {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    #[tokio::test]
    async fn lists_processes_from_numeric_directories_only() {
        let fake = FakeProc::new("list");
        fake.add_process(1, "init", 'S', None);
        fake.add_process(42, "nginx", 'R', None);
        // Non-numeric entries (e.g. /proc/self, /proc/meminfo) must be ignored.
        fs::create_dir_all(fake.root.join("self")).unwrap();

        let processes = fake.source().list_processes().await.unwrap();

        let mut pids: Vec<u32> = processes.iter().map(|p| p.pid).collect();
        pids.sort_unstable();
        assert_eq!(pids, vec![1, 42]);
    }

    #[tokio::test]
    async fn reads_command_state_and_memory() {
        let fake = FakeProc::new("fields");
        fake.add_process(7, "nginx", 'R', None);

        let processes = fake.source().list_processes().await.unwrap();
        let p = &processes[0];

        assert_eq!(p.pid, 7);
        assert_eq!(p.ppid, 1);
        assert_eq!(p.state, ProcessState::Running);
        // cmdline null bytes become spaces.
        assert_eq!(p.command, "nginx --flag");
        // 2 pages of 4096 bytes.
        assert_eq!(p.memory_bytes, 8192);
        // 8192 bytes out of a 1 MiB MemTotal.
        assert!((p.memory_percent - 0.78125).abs() < 1e-6);
    }

    #[tokio::test]
    async fn falls_back_to_comm_for_kernel_threads() {
        let fake = FakeProc::new("kthread");
        fake.add_process(2, "kthreadd", 'S', None);
        // Kernel threads expose an empty cmdline.
        fake.write("2/cmdline", "");

        let processes = fake.source().list_processes().await.unwrap();
        assert_eq!(processes[0].command, "kthreadd");
    }

    #[tokio::test]
    async fn detects_container_id_from_cgroup() {
        let fake = FakeProc::new("cgroup");
        fake.add_process(10, "app", 'R', Some("0::/docker/abc123def\n"));
        fake.add_process(
            11,
            "host-proc",
            'R',
            Some("0::/system.slice/sshd.service\n"),
        );

        let processes = fake.source().list_processes().await.unwrap();
        let contained = processes.iter().find(|p| p.pid == 10).unwrap();
        let plain = processes.iter().find(|p| p.pid == 11).unwrap();

        assert_eq!(
            contained.container_id.as_ref().map(|c| c.as_str()),
            Some("abc123def")
        );
        assert!(plain.container_id.is_none());
    }

    #[tokio::test]
    async fn strips_scope_suffix_from_cgroup_container_id() {
        let fake = FakeProc::new("scope");
        fake.add_process(12, "app", 'R', Some("0::/docker/deadbeef.scope\n"));

        let processes = fake.source().list_processes().await.unwrap();
        assert_eq!(
            processes[0].container_id.as_ref().map(|c| c.as_str()),
            Some("deadbeef")
        );
    }

    #[tokio::test]
    async fn unreadable_processes_are_skipped_not_fatal() {
        let fake = FakeProc::new("skip");
        fake.add_process(1, "ok", 'S', None);
        // A pid whose stat is truncated (or vanished mid-scan).
        fake.write("99/stat", "99 (gone) S 1");

        let processes = fake.source().list_processes().await.unwrap();
        assert_eq!(processes.len(), 1);
        assert_eq!(processes[0].pid, 1);
    }

    #[tokio::test]
    async fn missing_proc_directory_is_an_error() {
        let source = ProcfsProcessSource::new(ProcfsConfig::new(
            Path::new("/nonexistent-nanomon-proc"),
            Path::new("/sys"),
        ));
        assert!(source.list_processes().await.is_err());
    }

    #[tokio::test]
    async fn top_by_memory_sorts_desc_and_truncates() {
        let fake = FakeProc::new("top-mem");
        for (pid, pages) in [(1u32, "1"), (2, "9"), (3, "5")] {
            fake.add_process(pid, "p", 'S', None);
            let stat = fs::read_to_string(fake.root.join(format!("{pid}/stat"))).unwrap();
            // Replace the trailing rss field.
            let replaced = format!("{} {pages}", stat.trim_end().rsplit_once(' ').unwrap().0);
            fake.write(&format!("{pid}/stat"), &replaced);
        }

        let top = fake.source().get_top_by_memory(2).await.unwrap();
        assert_eq!(top.len(), 2);
        assert_eq!(top[0].pid, 2); // 9 pages
        assert_eq!(top[1].pid, 3); // 5 pages
    }

    #[tokio::test]
    async fn top_by_cpu_truncates_to_the_requested_count() {
        let fake = FakeProc::new("top-cpu");
        fake.add_process(1, "a", 'S', None);
        fake.add_process(2, "b", 'S', None);
        fake.add_process(3, "c", 'S', None);

        assert_eq!(fake.source().get_top_by_cpu(2).await.unwrap().len(), 2);
        // Asking for more than exists returns everything.
        assert_eq!(fake.source().get_top_by_cpu(10).await.unwrap().len(), 3);
    }
}
