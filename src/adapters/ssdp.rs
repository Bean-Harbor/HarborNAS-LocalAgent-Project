//! SSDP / UPnP discovery adapter boundary.

use std::collections::HashSet;
use std::net::{Ipv4Addr, SocketAddrV4, UdpSocket};
use std::time::{Duration, Instant};

use crate::runtime::discovery::{DiscoveryCandidate, DiscoveryRequest};
use crate::runtime::discovery::{DiscoveryCandidateStatus, DiscoveryProtocol};

pub const ADAPTER_NAME: &str = "ssdp";

pub trait SsdpDiscoveryAdapter: Send + Sync {
    fn discover(&self, request: &DiscoveryRequest) -> Result<Vec<DiscoveryCandidate>, String>;
}

const SSDP_DISCOVERY_ADDR: Ipv4Addr = Ipv4Addr::new(239, 255, 255, 250);
const SSDP_DISCOVERY_PORT: u16 = 1900;

#[derive(Debug, Clone)]
pub struct UdpSsdpAdapter {
    read_timeout: Duration,
    announce_window: Duration,
    search_target: String,
}

impl UdpSsdpAdapter {
    pub fn new(
        read_timeout: Duration,
        announce_window: Duration,
        search_target: impl Into<String>,
    ) -> Self {
        Self {
            read_timeout,
            announce_window,
            search_target: search_target.into(),
        }
    }

    fn build_msearch(st: &str) -> String {
        // https://datatracker.ietf.org/doc/html/draft-cai-ssdp-v1-03 (legacy formatting)
        format!(
            "M-SEARCH * HTTP/1.1\r\nHOST: 239.255.255.250:1900\r\nMAN: \"ssdp:discover\"\r\nMX: 1\r\nST: {st}\r\nUSER-AGENT: harborbeacon-agent-hub/0.1\r\n\r\n"
        )
    }

    fn parse_ssdp_response(payload: &[u8]) -> Option<ParsedSsdpResponse> {
        let text = std::str::from_utf8(payload).ok()?;
        let mut usn: Option<String> = None;
        let mut server: Option<String> = None;
        let mut location: Option<String> = None;
        let mut st: Option<String> = None;

        for line in text.lines().skip(1) {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let (key, value) = line.split_once(':')?;
            let key = key.trim().to_ascii_lowercase();
            let value = value.trim().to_string();
            match key.as_str() {
                "usn" => usn = Some(value),
                "server" => server = Some(value),
                "location" => location = Some(value),
                "st" => st = Some(value),
                _ => {}
            }
        }

        Some(ParsedSsdpResponse {
            usn,
            server,
            location,
            st,
        })
    }
}

impl Default for UdpSsdpAdapter {
    fn default() -> Self {
        Self::new(
            Duration::from_millis(650),
            Duration::from_millis(1200),
            "ssdp:all",
        )
    }
}

impl SsdpDiscoveryAdapter for UdpSsdpAdapter {
    fn discover(&self, request: &DiscoveryRequest) -> Result<Vec<DiscoveryCandidate>, String> {
        let (network, prefix) = parse_ipv4_cidr(&request.network_cidr)?;
        let socket = UdpSocket::bind(SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, 0))
            .map_err(|e| format!("SSDP discovery bind failed: {e}"))?;
        socket
            .set_read_timeout(Some(self.read_timeout))
            .map_err(|e| format!("SSDP discovery read timeout failed: {e}"))?;
        socket
            .set_multicast_loop_v4(false)
            .map_err(|e| format!("SSDP discovery multicast loop config failed: {e}"))?;

        let payload = Self::build_msearch(&self.search_target);
        let target = SocketAddrV4::new(SSDP_DISCOVERY_ADDR, SSDP_DISCOVERY_PORT);
        for _ in 0..2 {
            let _ = socket.send_to(payload.as_bytes(), target);
        }

        let deadline = Instant::now() + self.announce_window;
        let mut seen = HashSet::new();
        let mut candidates = Vec::new();

        while Instant::now() < deadline {
            let mut buf = [0u8; 8 * 1024];
            let (size, source) = match socket.recv_from(&mut buf) {
                Ok(value) => value,
                Err(error)
                    if matches!(
                        error.kind(),
                        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                    ) =>
                {
                    continue;
                }
                Err(error) => return Err(format!("SSDP discovery receive failed: {error}")),
            };

            let source_ip = match source {
                std::net::SocketAddr::V4(addr) => *addr.ip(),
                std::net::SocketAddr::V6(_) => continue,
            };
            if !ipv4_in_cidr(source_ip, network, prefix) {
                continue;
            }

            let parsed = Self::parse_ssdp_response(&buf[..size]);
            let candidate_id = parsed
                .as_ref()
                .and_then(|value| value.usn.as_ref())
                .cloned()
                .unwrap_or_else(|| format!("ssdp-{}", source_ip.to_string().replace('.', "-")));
            if !seen.insert(candidate_id.clone()) {
                continue;
            }

            let (vendor, model) = parsed
                .as_ref()
                .and_then(|value| value.server.as_ref())
                .map(|server| (Some(server.clone()), None))
                .unwrap_or((None, None));

            let name = parsed.as_ref().and_then(|value| value.st.as_ref()).cloned();

            candidates.push(DiscoveryCandidate {
                candidate_id,
                protocol: DiscoveryProtocol::Ssdp,
                name,
                ip_address: source_ip.to_string(),
                port: None,
                vendor,
                model,
                rtsp_paths: Vec::new(),
                status: DiscoveryCandidateStatus::Discovered,
            });
        }

        Ok(candidates)
    }
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
struct ParsedSsdpResponse {
    usn: Option<String>,
    server: Option<String>,
    location: Option<String>,
    st: Option<String>,
}

fn parse_ipv4_cidr(cidr: &str) -> Result<(Ipv4Addr, u8), String> {
    let mut parts = cidr.trim().split('/');
    let network = parts
        .next()
        .ok_or_else(|| format!("invalid CIDR: {cidr}"))?
        .parse::<Ipv4Addr>()
        .map_err(|e| format!("invalid CIDR network {cidr}: {e}"))?;
    let prefix = parts
        .next()
        .ok_or_else(|| format!("invalid CIDR prefix: {cidr}"))?
        .parse::<u8>()
        .map_err(|e| format!("invalid CIDR prefix {cidr}: {e}"))?;
    if prefix > 32 {
        return Err(format!("CIDR prefix out of range: {cidr}"));
    }
    Ok((network, prefix))
}

fn ipv4_in_cidr(ip: Ipv4Addr, network: Ipv4Addr, prefix: u8) -> bool {
    let mask = if prefix == 0 {
        0
    } else {
        u32::MAX << (32 - prefix)
    };
    (u32::from(ip) & mask) == (u32::from(network) & mask)
}

#[cfg(test)]
mod tests {
    use super::UdpSsdpAdapter;

    #[test]
    fn parses_basic_ssdp_response_headers() {
        let payload = b"HTTP/1.1 200 OK\r\nUSN: uuid:demo::upnp:rootdevice\r\nST: upnp:rootdevice\r\nSERVER: demo/1.0 UPnP/1.1\r\nLOCATION: http://192.168.1.10:80/desc.xml\r\n\r\n";
        let parsed = UdpSsdpAdapter::parse_ssdp_response(payload).expect("parsed");
        assert_eq!(parsed.usn.as_deref(), Some("uuid:demo::upnp:rootdevice"));
        assert_eq!(parsed.st.as_deref(), Some("upnp:rootdevice"));
        assert_eq!(parsed.server.as_deref(), Some("demo/1.0 UPnP/1.1"));
        assert_eq!(
            parsed.location.as_deref(),
            Some("http://192.168.1.10:80/desc.xml")
        );
    }
}
