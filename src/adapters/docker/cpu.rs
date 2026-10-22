use std::collections::HashMap;

use bollard::models::ContainerStatsResponse;

use crate::domain::ContainerId;

/// Cumulative CPU counters of one container, read from one stats sample.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CpuSample {
    /// CPU time the container consumed since it started.
    container_ns: u64,
    /// CPU time the whole host consumed since boot.
    host_ns: u64,
    /// CPUs the container may run on.
    online_cpus: u32,
}

/// Last CPU sample seen for each container.
///
/// Docker's counters are cumulative, so the current load is the difference
/// between two samples. A one-shot stats call carries no previous sample, so
/// the adapter keeps the last one itself, as the procfs adapter does for the
/// host CPU.
#[derive(Debug, Default)]
pub struct CpuSamples {
    last: HashMap<ContainerId, CpuSample>,
}

impl CpuSamples {
    /// Records a sample and returns the container's CPU usage since the
    /// previous one.
    ///
    /// # Arguments
    /// * `id` - Container the sample belongs to.
    /// * `sample` - Its counters at this collection.
    ///
    /// # Returns
    /// The usage in percent of one CPU (400% means four busy CPUs), or 0 when
    /// there is no previous sample to compare with.
    pub fn record(&mut self, id: ContainerId, sample: CpuSample) -> f64 {
        // `insert` hands back the value it replaced, i.e. the previous sample.
        // `CpuSample` is `Copy`, so `sample` stays usable after the insert.
        match self.last.insert(id, sample) {
            Some(previous) => sample.usage_percent_since(&previous),
            None => 0.0,
        }
    }

    /// Forgets the containers that are no longer listed.
    ///
    /// Containers are recreated with a new id on every image update, so
    /// without this the map would grow for as long as NanoMon runs.
    ///
    /// # Arguments
    /// * `listed` - Containers present in the latest listing.
    pub fn keep_only(&mut self, listed: &[ContainerId]) {
        self.last.retain(|id, _| listed.contains(id));
    }
}

impl CpuSample {
    /// Reads the CPU counters of a Docker stats sample.
    ///
    /// # Arguments
    /// * `stats` - One sample from the Docker stats endpoint.
    ///
    /// # Returns
    /// The counters, with an absent one read as 0 and an absent CPU count as 1.
    pub fn from_stats(stats: &ContainerStatsResponse) -> Self {
        // `as_ref` borrows inside the Option instead of moving the counters out.
        let cpu = stats.cpu_stats.as_ref();
        Self {
            container_ns: cpu
                .and_then(|c| c.cpu_usage.as_ref())
                .and_then(|u| u.total_usage)
                .unwrap_or(0),
            host_ns: cpu.and_then(|c| c.system_cpu_usage).unwrap_or(0),
            online_cpus: cpu.and_then(|c| c.online_cpus).unwrap_or(1),
        }
    }

    /// CPU usage between `previous` and this sample.
    fn usage_percent_since(&self, previous: &CpuSample) -> f64 {
        // Counters reset when a container restarts: saturate rather than wrap.
        let container_delta = self.container_ns.saturating_sub(previous.container_ns) as f64;
        let host_delta = self.host_ns.saturating_sub(previous.host_ns) as f64;
        if host_delta == 0.0 {
            return 0.0;
        }
        container_delta / host_delta * f64::from(self.online_cpus) * 100.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(container_ns: u64, host_ns: u64) -> CpuSample {
        CpuSample {
            container_ns,
            host_ns,
            online_cpus: 4,
        }
    }

    fn docker_sample(total_usage: u64, system_cpu_usage: u64) -> ContainerStatsResponse {
        let json = format!(
            r#"{{ "cpu_stats": {{
                "cpu_usage": {{ "total_usage": {total_usage} }},
                "system_cpu_usage": {system_cpu_usage}, "online_cpus": 2 }} }}"#
        );
        serde_json::from_str(&json).expect("fixture matches the Docker stats schema")
    }

    #[test]
    fn samples_are_read_from_the_docker_cpu_counters() {
        let mut samples = CpuSamples::default();
        let id = ContainerId::new("web");
        samples.record(
            id.clone(),
            CpuSample::from_stats(&docker_sample(1_000, 20_000)),
        );

        // (3000 - 1000) / (40000 - 20000) * 2 cpus * 100
        let usage = samples.record(id, CpuSample::from_stats(&docker_sample(3_000, 40_000)));

        assert!((usage - 20.0).abs() < 1e-9, "got {usage}");
    }

    #[test]
    fn the_first_sample_of_a_container_reports_no_usage() {
        let mut samples = CpuSamples::default();

        let usage = samples.record(ContainerId::new("web"), sample(3_000, 40_000));

        assert_eq!(usage, 0.0);
    }

    #[test]
    fn usage_is_the_share_of_host_time_since_the_previous_sample() {
        let mut samples = CpuSamples::default();
        samples.record(ContainerId::new("web"), sample(1_000, 20_000));

        // (3000 - 1000) / (40000 - 20000) * 4 cpus * 100
        let usage = samples.record(ContainerId::new("web"), sample(3_000, 40_000));

        assert!((usage - 40.0).abs() < 1e-9, "got {usage}");
    }

    #[test]
    fn containers_no_longer_listed_are_forgotten() {
        let mut samples = CpuSamples::default();
        samples.record(ContainerId::new("gone"), sample(1_000, 20_000));
        samples.record(ContainerId::new("kept"), sample(1_000, 20_000));

        samples.keep_only(&[ContainerId::new("kept")]);

        // "gone" starts over without a reference, "kept" still has one.
        let gone = samples.record(ContainerId::new("gone"), sample(3_000, 40_000));
        let kept = samples.record(ContainerId::new("kept"), sample(3_000, 40_000));
        assert_eq!(gone, 0.0);
        assert!(kept > 0.0);
    }

    #[test]
    fn two_samples_of_the_same_instant_report_no_usage() {
        // Two API requests can hit Docker within the same host tick.
        let mut samples = CpuSamples::default();
        samples.record(ContainerId::new("web"), sample(3_000, 40_000));

        let usage = samples.record(ContainerId::new("web"), sample(3_000, 40_000));

        assert_eq!(usage, 0.0);
    }

    #[test]
    fn a_restarted_container_reports_no_usage_rather_than_a_wrapped_counter() {
        let mut samples = CpuSamples::default();
        samples.record(ContainerId::new("web"), sample(900_000, 20_000));

        // The restart reset the container's counter below the previous value.
        let usage = samples.record(ContainerId::new("web"), sample(1_000, 40_000));

        assert_eq!(usage, 0.0);
    }
}
