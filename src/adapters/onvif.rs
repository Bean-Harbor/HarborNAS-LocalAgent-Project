//! ONVIF discovery and camera control adapter boundary.

use std::collections::HashSet;
use std::net::{Ipv4Addr, SocketAddrV4, UdpSocket};
use std::time::{Duration, Instant};

use quick_xml::events::Event;
use quick_xml::Reader;
use uuid::Uuid;

use crate::runtime::discovery::{
    DiscoveryCandidate, DiscoveryCandidateStatus, DiscoveryProtocol, DiscoveryRequest,
};

pub const ADAPTER_NAME: &str = "onvif";

const ONVIF_DISCOVERY_ADDR: Ipv4Addr = Ipv4Addr::new(239, 255, 255, 250);
const ONVIF_DISCOVERY_PORT: u16 = 3702;

pub trait OnvifDiscoveryAdapter: Send + Sync {
    fn discover(&self, request: &DiscoveryRequest) -> Result<Vec<DiscoveryCandidate>, String>;
}

#[derive(Debug, Clone)]
pub struct WsDiscoveryOnvifAdapter {
    read_timeout: Duration,
    announce_window: Duration,
}

impl WsDiscoveryOnvifAdapter {
    pub fn new(read_timeout: Duration, announce_window: Duration) -> Self {
        Self {
            read_timeout,
            announce_window,
        }
    }

    fn build_probe_message() -> String {
        let message_id = format!("uuid:{}", Uuid::new_v4());
        format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<e:Envelope xmlns:e="http://www.w3.org/2003/05/soap-envelope"
            xmlns:w="http://schemas.xmlsoap.org/ws/2004/08/addressing"
            xmlns:d="http://schemas.xmlsoap.org/ws/2005/04/discovery"
            xmlns:dn="http://www.onvif.org/ver10/network/wsdl">
  <e:Header>
    <w:MessageID>{message_id}</w:MessageID>
    <w:To>urn:schemas-xmlsoap-org:ws:2005:04:discovery</w:To>
    <w:Action>http://schemas.xmlsoap.org/ws/2005/04/discovery/Probe</w:Action>
  </e:Header>
  <e:Body>
    <d:Probe>
      <d:Types>dn:NetworkVideoTransmitter</d:Types>
    </d:Probe>
  </e:Body>
</e:Envelope>"#
        )
    }
}

impl Default for WsDiscoveryOnvifAdapter {
    fn default() -> Self {
        Self::new(Duration::from_millis(500), Duration::from_millis(1200))
    }
}

impl OnvifDiscoveryAdapter for WsDiscoveryOnvifAdapter {
    fn discover(&self, request: &DiscoveryRequest) -> Result<Vec<DiscoveryCandidate>, String> {
        let (network, prefix) = parse_ipv4_cidr(&request.network_cidr)?;
        let socket = UdpSocket::bind(SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, 0))
            .map_err(|e| format!("ONVIF discovery bind failed: {e}"))?;
        socket
            .set_read_timeout(Some(self.read_timeout))
            .map_err(|e| format!("ONVIF discovery read timeout failed: {e}"))?;
        socket
            .set_multicast_loop_v4(false)
            .map_err(|e| format!("ONVIF discovery multicast loop config failed: {e}"))?;

        let payload = Self::build_probe_message();
        let target = SocketAddrV4::new(ONVIF_DISCOVERY_ADDR, ONVIF_DISCOVERY_PORT);
        for _ in 0..2 {
            let _ = socket.send_to(payload.as_bytes(), target);
        }

        let deadline = Instant::now() + self.announce_window;
        let mut seen = HashSet::new();
        let mut candidates = Vec::new();

        while Instant::now() < deadline {
            let mut buf = [0u8; 16 * 1024];
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
                Err(error) => return Err(format!("ONVIF discovery receive failed: {error}")),
            };

            let source_ip = match source {
                std::net::SocketAddr::V4(addr) => *addr.ip(),
                std::net::SocketAddr::V6(_) => continue,
            };
            if !ipv4_in_cidr(source_ip, network, prefix) {
                continue;
            }

            if let Some(candidate) = parse_probe_match(&buf[..size], source_ip) {
                let unique_key = if candidate.candidate_id.trim().is_empty() {
                    candidate.ip_address.clone()
                } else {
                    candidate.candidate_id.clone()
                };
                if seen.insert(unique_key) {
                    candidates.push(candidate);
                }
            }
        }

        Ok(candidates)
    }
}

fn parse_probe_match(payload: &[u8], fallback_ip: Ipv4Addr) -> Option<DiscoveryCandidate> {
    let text = std::str::from_utf8(payload).ok()?;
    let probe = parse_onvif_probe_xml(text)?;
    let ip = probe
        .xaddrs
        .iter()
        .find_map(|xaddr| extract_ipv4_from_url(xaddr))
        .unwrap_or(fallback_ip);
    let name = scope_value(&probe.scopes, "/name/");
    let model = scope_value(&probe.scopes, "/hardware/");
    let vendor = scope_value(&probe.scopes, "/manufacturer/");
    let candidate_id = probe
        .endpoint_reference
        .clone()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| format!("onvif-{}", ip.to_string().replace('.', "-")));

    Some(DiscoveryCandidate {
        candidate_id,
        protocol: DiscoveryProtocol::Onvif,
        name,
        ip_address: ip.to_string(),
        port: None,
        vendor,
        model,
        rtsp_paths: Vec::new(),
        status: DiscoveryCandidateStatus::Discovered,
    })
}

#[derive(Debug, Default, PartialEq, Eq)]
struct ParsedProbeMatch {
    endpoint_reference: Option<String>,
    xaddrs: Vec<String>,
    scopes: Vec<String>,
}

fn parse_onvif_probe_xml(xml: &str) -> Option<ParsedProbeMatch> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);

    let mut current_tag = String::new();
    let mut parsed = ParsedProbeMatch::default();

    loop {
        match reader.read_event() {
            Ok(Event::Start(event)) => {
                current_tag = String::from_utf8_lossy(event.local_name().as_ref()).to_string();
            }
            Ok(Event::Text(text)) => {
                let value = text.decode().ok()?.trim().to_string();
                if value.is_empty() {
                    continue;
                }
                match current_tag.as_str() {
                    "Address" => parsed.endpoint_reference = Some(value),
                    "XAddrs" => {
                        parsed.xaddrs = value
                            .split_whitespace()
                            .map(|item| item.to_string())
                            .collect();
                    }
                    "Scopes" => {
                        parsed.scopes = value
                            .split_whitespace()
                            .map(|item| item.to_string())
                            .collect();
                    }
                    _ => {}
                }
            }
            Ok(Event::End(_)) => {
                current_tag.clear();
            }
            Ok(Event::Eof) => break,
            Err(_) => return None,
            _ => {}
        }
    }

    (!parsed.xaddrs.is_empty() || parsed.endpoint_reference.is_some()).then_some(parsed)
}

fn scope_value(scopes: &[String], needle: &str) -> Option<String> {
    scopes.iter().find_map(|scope| {
        let value = scope.split_once(needle)?.1;
        let decoded = value.replace("%20", " ").replace('_', " ");
        Some(decoded)
    })
}

fn extract_ipv4_from_url(url: &str) -> Option<Ipv4Addr> {
    let parsed = reqwest::Url::parse(url).ok()?;
    parsed.host_str()?.parse().ok()
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
    use std::net::Ipv4Addr;

    use super::{extract_ipv4_from_url, parse_onvif_probe_xml, parse_probe_match, scope_value};

    const PROBE_MATCH: &str = r#"
<e:Envelope xmlns:e="http://www.w3.org/2003/05/soap-envelope"
            xmlns:a="http://schemas.xmlsoap.org/ws/2004/08/addressing"
            xmlns:d="http://schemas.xmlsoap.org/ws/2005/04/discovery">
  <e:Body>
    <d:ProbeMatches>
      <d:ProbeMatch>
        <a:EndpointReference>
          <a:Address>urn:uuid:2d9f8f2c-0b11-11ee-be56-0242ac120002</a:Address>
        </a:EndpointReference>
        <d:Types>dn:NetworkVideoTransmitter tds:Device</d:Types>
        <d:Scopes>onvif://www.onvif.org/name/Living_Room_Cam onvif://www.onvif.org/hardware/DS-2CD onvif://www.onvif.org/manufacturer/Hikvision</d:Scopes>
        <d:XAddrs>http://192.168.3.73/onvif/device_service</d:XAddrs>
        <d:MetadataVersion>1</d:MetadataVersion>
      </d:ProbeMatch>
    </d:ProbeMatches>
  </e:Body>
</e:Envelope>
"#;

    #[test]
    fn parse_probe_xml_extracts_core_fields() {
        let parsed = parse_onvif_probe_xml(PROBE_MATCH).expect("probe");
        assert_eq!(
            parsed.endpoint_reference.as_deref(),
            Some("urn:uuid:2d9f8f2c-0b11-11ee-be56-0242ac120002")
        );
        assert_eq!(
            parsed.xaddrs,
            vec!["http://192.168.3.73/onvif/device_service".to_string()]
        );
        assert_eq!(parsed.scopes.len(), 3);
    }

    #[test]
    fn parse_probe_match_builds_candidate() {
        let candidate = parse_probe_match(PROBE_MATCH.as_bytes(), Ipv4Addr::new(192, 168, 3, 90))
            .expect("candidate");
        assert_eq!(candidate.ip_address, "192.168.3.73");
        assert_eq!(candidate.name.as_deref(), Some("Living Room Cam"));
        assert_eq!(candidate.model.as_deref(), Some("DS-2CD"));
        assert_eq!(candidate.vendor.as_deref(), Some("Hikvision"));
        assert!(candidate.candidate_id.contains("urn:uuid:"));
    }

    #[test]
    fn scope_value_decodes_scope_suffixes() {
        let scopes = vec!["onvif://www.onvif.org/name/Front_Door_Cam".to_string()];
        assert_eq!(
            scope_value(&scopes, "/name/").as_deref(),
            Some("Front Door Cam")
        );
    }

    #[test]
    fn extract_ipv4_from_url_reads_host() {
        assert_eq!(
            extract_ipv4_from_url("http://192.168.1.60/onvif/device_service"),
            Some(Ipv4Addr::new(192, 168, 1, 60))
        );
    }
}
