//! Container source for a host that has no Docker at all.
//!
//! NanoMon's primary use case is a NAS running Docker, but it also runs on
//! plain servers — a seedbox, a VPS — where there is no daemon to talk to and
//! never will be. Refusing to start there costs the operator every other metric
//! NanoMon collects (CPU, memory, disks, network, systemd services) over the one
//! source that happens to be absent.

use async_trait::async_trait;

use crate::domain::{Container, ContainerId};
use crate::ports::{ContainerSource, ContainerStats};

/// A `ContainerSource` that reports no containers, ever.
///
/// A Null Object rather than an `Option<Arc<dyn ContainerSource>>` threaded
/// through the monitoring service: the service should not have to know whether
/// containers are observable, only that the list may be empty — which it must
/// already handle for a host that simply runs none.
#[derive(Debug, Default, Clone, Copy)]
pub struct NoContainers;

#[async_trait]
impl ContainerSource for NoContainers {
    async fn list_containers(
        &self,
    ) -> Result<Vec<Container>, Box<dyn std::error::Error + Send + Sync>> {
        Ok(Vec::new())
    }

    /// # Errors
    ///
    /// Always: an id can only have come from a list this source never produced.
    async fn get_container_stats(
        &self,
        id: &ContainerId,
    ) -> Result<ContainerStats, Box<dyn std::error::Error + Send + Sync>> {
        Err(format!("no container runtime on this host (asked for {id:?})").into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn lists_nothing_instead_of_failing() {
        let source = NoContainers;

        let containers = source.list_containers().await.expect("must not fail");

        assert!(containers.is_empty());
    }
}
