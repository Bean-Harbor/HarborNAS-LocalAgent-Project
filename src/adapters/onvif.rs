//! ONVIF discovery and camera control adapter boundary.

use crate::runtime::discovery::{DiscoveryCandidate, DiscoveryRequest};

pub const ADAPTER_NAME: &str = "onvif";

pub trait OnvifDiscoveryAdapter: Send + Sync {
    fn discover(&self, request: &DiscoveryRequest) -> Result<Vec<DiscoveryCandidate>, String>;
}
