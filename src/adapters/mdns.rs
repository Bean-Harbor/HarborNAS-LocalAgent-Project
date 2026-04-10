//! mDNS discovery adapter boundary.

use crate::runtime::discovery::{DiscoveryCandidate, DiscoveryRequest};

pub const ADAPTER_NAME: &str = "mdns";

pub trait MdnsDiscoveryAdapter: Send + Sync {
    fn discover(&self, request: &DiscoveryRequest) -> Result<Vec<DiscoveryCandidate>, String>;
}
