//! RTSP media stream adapter boundary.

use crate::runtime::discovery::{RtspProbeRequest, RtspProbeResult};

pub const ADAPTER_NAME: &str = "rtsp";

pub trait RtspProbeAdapter: Send + Sync {
    fn probe(&self, request: &RtspProbeRequest) -> Result<RtspProbeResult, String>;
}
