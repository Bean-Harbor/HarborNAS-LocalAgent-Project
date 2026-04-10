//! SSDP / UPnP discovery adapter boundary.

use crate::runtime::discovery::{DiscoveryCandidate, DiscoveryRequest};

pub const ADAPTER_NAME: &str = "ssdp";

pub trait SsdpDiscoveryAdapter: Send + Sync {
    fn discover(&self, request: &DiscoveryRequest) -> Result<Vec<DiscoveryCandidate>, String>;
}
