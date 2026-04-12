//! ONVIF discovery and camera control adapter boundary.

use std::collections::HashSet;
use std::net::{Ipv4Addr, SocketAddrV4, UdpSocket};
use std::time::{Duration, Instant};

use quick_xml::events::Event;
use quick_xml::Reader;
use reqwest::blocking::Client;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue, CONTENT_TYPE};
use uuid::Uuid;

use crate::runtime::discovery::{
    DiscoveryCandidate, DiscoveryCandidateStatus, DiscoveryProtocol, DiscoveryRequest,
};
use crate::runtime::registry::CameraDevice;

pub const ADAPTER_NAME: &str = "onvif";

const ONVIF_DISCOVERY_ADDR: Ipv4Addr = Ipv4Addr::new(239, 255, 255, 250);
const ONVIF_DISCOVERY_PORT: u16 = 3702;

pub trait OnvifDiscoveryAdapter: Send + Sync {
    fn discover(&self, request: &DiscoveryRequest) -> Result<Vec<DiscoveryCandidate>, String>;
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PtzDirection {
    Left,
    Right,
    Up,
    Down,
    Stop,
}

#[derive(Debug, Clone, PartialEq)]
pub struct OnvifPtzRequest {
    pub device_service_url: String,
    pub username: Option<String>,
    pub password: Option<String>,
    pub direction: PtzDirection,
    pub pan_speed: f32,
    pub tilt_speed: f32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OnvifPtzResult {
    pub profile_token: String,
    pub ptz_service_url: String,
    pub action: String,
}

pub trait OnvifPtzAdapter: Send + Sync {
    fn ptz(&self, request: &OnvifPtzRequest) -> Result<OnvifPtzResult, String>;
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

#[derive(Debug, Clone)]
pub struct SoapOnvifPtzAdapter {
    client: Client,
}

impl SoapOnvifPtzAdapter {
    pub fn new(timeout: Duration) -> Result<Self, String> {
        let client = Client::builder()
            .timeout(timeout)
            .build()
            .map_err(|e| format!("failed to build ONVIF PTZ client: {e}"))?;
        Ok(Self { client })
    }

    fn post_soap(
        &self,
        url: &str,
        action: &str,
        body: &str,
    ) -> Result<String, String> {
        let mut headers = HeaderMap::new();
        headers.insert(
            CONTENT_TYPE,
            HeaderValue::from_static("application/soap+xml; charset=utf-8"),
        );
        headers.insert(
            HeaderName::from_static("soapaction"),
            HeaderValue::from_str(action).map_err(|e| format!("invalid SOAP action: {e}"))?,
        );

        self.client
            .post(url)
            .headers(headers)
            .body(body.to_string())
            .send()
            .and_then(|resp| resp.error_for_status())
            .map_err(|e| format!("ONVIF SOAP request failed: {e}"))?
            .text()
            .map_err(|e| format!("ONVIF SOAP response decode failed: {e}"))
    }
}

impl Default for SoapOnvifPtzAdapter {
    fn default() -> Self {
        Self::new(Duration::from_secs(5)).expect("build ONVIF PTZ client")
    }
}

impl OnvifPtzAdapter for SoapOnvifPtzAdapter {
    fn ptz(&self, request: &OnvifPtzRequest) -> Result<OnvifPtzResult, String> {
        let capabilities_xml = build_get_capabilities_envelope(
            request.username.as_deref(),
            request.password.as_deref(),
        );
        let capabilities = self.post_soap(
            &request.device_service_url,
            "http://www.onvif.org/ver10/device/wsdl/GetCapabilities",
            &capabilities_xml,
        )?;
        let media_url = parse_xaddr(&capabilities, "Media")
            .unwrap_or_else(|| request.device_service_url.clone());
        let ptz_url = parse_xaddr(&capabilities, "PTZ")
            .unwrap_or_else(|| request.device_service_url.clone());

        let profiles_xml = build_get_profiles_envelope(
            request.username.as_deref(),
            request.password.as_deref(),
        );
        let profiles = self.post_soap(
            &media_url,
            "http://www.onvif.org/ver10/media/wsdl/GetProfiles",
            &profiles_xml,
        )?;
        let profile_token = parse_first_profile_token(&profiles)
            .ok_or_else(|| "ONVIF GetProfiles returned no profile token".to_string())?;

        match request.direction {
            PtzDirection::Stop => {
                let stop_xml = build_stop_envelope(
                    request.username.as_deref(),
                    request.password.as_deref(),
                    &profile_token,
                );
                self.post_soap(
                    &ptz_url,
                    "http://www.onvif.org/ver20/ptz/wsdl/Stop",
                    &stop_xml,
                )?;
                Ok(OnvifPtzResult {
                    profile_token,
                    ptz_service_url: ptz_url,
                    action: "stop".to_string(),
                })
            }
            _ => {
                let (x, y) = request.direction.velocity(request.pan_speed, request.tilt_speed);
                let move_xml = build_continuous_move_envelope(
                    request.username.as_deref(),
                    request.password.as_deref(),
                    &profile_token,
                    x,
                    y,
                );
                self.post_soap(
                    &ptz_url,
                    "http://www.onvif.org/ver20/ptz/wsdl/ContinuousMove",
                    &move_xml,
                )?;
                Ok(OnvifPtzResult {
                    profile_token,
                    ptz_service_url: ptz_url,
                    action: request.direction.as_str().to_string(),
                })
            }
        }
    }
}

impl PtzDirection {
    pub fn as_str(&self) -> &'static str {
        match self {
            PtzDirection::Left => "left",
            PtzDirection::Right => "right",
            PtzDirection::Up => "up",
            PtzDirection::Down => "down",
            PtzDirection::Stop => "stop",
        }
    }

    fn velocity(&self, pan_speed: f32, tilt_speed: f32) -> (f32, f32) {
        match self {
            PtzDirection::Left => (-pan_speed.abs(), 0.0),
            PtzDirection::Right => (pan_speed.abs(), 0.0),
            PtzDirection::Up => (0.0, tilt_speed.abs()),
            PtzDirection::Down => (0.0, -tilt_speed.abs()),
            PtzDirection::Stop => (0.0, 0.0),
        }
    }
}

pub fn default_onvif_device_service_url(device: &CameraDevice) -> Option<String> {
    device
        .onvif_device_service_url
        .clone()
        .or_else(|| {
            device
                .ip_address
                .as_ref()
                .map(|ip| format!("http://{ip}/onvif/device_service"))
        })
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

fn build_get_capabilities_envelope(username: Option<&str>, password: Option<&str>) -> String {
    build_soap_envelope(
        username,
        password,
        r#"<tds:GetCapabilities xmlns:tds="http://www.onvif.org/ver10/device/wsdl"><tds:Category>All</tds:Category></tds:GetCapabilities>"#,
    )
}

fn build_get_profiles_envelope(username: Option<&str>, password: Option<&str>) -> String {
    build_soap_envelope(
        username,
        password,
        r#"<trt:GetProfiles xmlns:trt="http://www.onvif.org/ver10/media/wsdl"/>"#,
    )
}

fn build_continuous_move_envelope(
    username: Option<&str>,
    password: Option<&str>,
    profile_token: &str,
    x: f32,
    y: f32,
) -> String {
    build_soap_envelope(
        username,
        password,
        &format!(
            r#"<tptz:ContinuousMove xmlns:tptz="http://www.onvif.org/ver20/ptz/wsdl">
<tptz:ProfileToken>{profile_token}</tptz:ProfileToken>
<tptz:Velocity>
  <tt:PanTilt xmlns:tt="http://www.onvif.org/ver10/schema" x="{x:.3}" y="{y:.3}"/>
</tptz:Velocity>
</tptz:ContinuousMove>"#
        ),
    )
}

fn build_stop_envelope(
    username: Option<&str>,
    password: Option<&str>,
    profile_token: &str,
) -> String {
    build_soap_envelope(
        username,
        password,
        &format!(
            r#"<tptz:Stop xmlns:tptz="http://www.onvif.org/ver20/ptz/wsdl">
<tptz:ProfileToken>{profile_token}</tptz:ProfileToken>
<tptz:PanTilt>true</tptz:PanTilt>
<tptz:Zoom>true</tptz:Zoom>
</tptz:Stop>"#
        ),
    )
}

fn build_soap_envelope(username: Option<&str>, password: Option<&str>, body: &str) -> String {
    let security = build_wsse_header(username, password);
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<s:Envelope xmlns:s="http://www.w3.org/2003/05/soap-envelope">
  <s:Header>{security}</s:Header>
  <s:Body>{body}</s:Body>
</s:Envelope>"#
    )
}

fn build_wsse_header(username: Option<&str>, password: Option<&str>) -> String {
    match (username, password) {
        (Some(username), Some(password)) if !username.trim().is_empty() => format!(
            r#"<wsse:Security xmlns:wsse="http://docs.oasis-open.org/wss/2004/01/oasis-200401-wss-wssecurity-secext-1.0.xsd">
  <wsse:UsernameToken>
    <wsse:Username>{}</wsse:Username>
    <wsse:Password>{}</wsse:Password>
  </wsse:UsernameToken>
</wsse:Security>"#,
            xml_escape(username),
            xml_escape(password)
        ),
        _ => String::new(),
    }
}

fn xml_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn parse_xaddr(xml: &str, service_name: &str) -> Option<String> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);
    let mut current_tag = String::new();
    let mut in_service = false;
    loop {
        match reader.read_event() {
            Ok(Event::Start(event)) => {
                current_tag = String::from_utf8_lossy(event.local_name().as_ref()).to_string();
                if current_tag == service_name {
                    in_service = true;
                }
            }
            Ok(Event::Text(text)) if in_service && current_tag == "XAddr" => {
                let value = text.decode().ok()?.trim().to_string();
                if !value.is_empty() {
                    return Some(value);
                }
            }
            Ok(Event::End(event)) => {
                let tag = String::from_utf8_lossy(event.local_name().as_ref()).to_string();
                if tag == service_name {
                    in_service = false;
                }
                current_tag.clear();
            }
            Ok(Event::Eof) => break,
            Err(_) => return None,
            _ => {}
        }
    }
    None
}

fn parse_first_profile_token(xml: &str) -> Option<String> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);
    loop {
        match reader.read_event() {
            Ok(Event::Start(event)) | Ok(Event::Empty(event)) => {
                let local = event.local_name();
                if local.as_ref() == b"Profiles" || local.as_ref() == b"Profile" {
                    for attr in event.attributes().flatten() {
                        if attr.key.local_name().as_ref() == b"token" {
                            return Some(String::from_utf8_lossy(attr.value.as_ref()).to_string());
                        }
                    }
                }
            }
            Ok(Event::Eof) => break,
            Err(_) => return None,
            _ => {}
        }
    }
    None
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

    use super::{
        build_continuous_move_envelope, build_stop_envelope, extract_ipv4_from_url,
        parse_first_profile_token, parse_onvif_probe_xml, parse_probe_match, parse_xaddr,
        scope_value,
    };

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

    #[test]
    fn parse_xaddr_reads_media_service_url() {
        let xml = r#"
<s:Envelope xmlns:s="http://www.w3.org/2003/05/soap-envelope">
  <s:Body>
    <tds:GetCapabilitiesResponse xmlns:tds="http://www.onvif.org/ver10/device/wsdl">
      <tds:Capabilities>
        <tt:Media xmlns:tt="http://www.onvif.org/ver10/schema">
          <tt:XAddr>http://192.168.3.73/onvif/media_service</tt:XAddr>
        </tt:Media>
      </tds:Capabilities>
    </tds:GetCapabilitiesResponse>
  </s:Body>
</s:Envelope>"#;
        assert_eq!(
            parse_xaddr(xml, "Media").as_deref(),
            Some("http://192.168.3.73/onvif/media_service")
        );
    }

    #[test]
    fn parse_first_profile_token_reads_first_profile() {
        let xml = r#"
<s:Envelope xmlns:s="http://www.w3.org/2003/05/soap-envelope">
  <s:Body>
    <trt:GetProfilesResponse xmlns:trt="http://www.onvif.org/ver10/media/wsdl">
      <trt:Profiles token="profile_1" fixed="true" />
      <trt:Profiles token="profile_2" fixed="false" />
    </trt:GetProfilesResponse>
  </s:Body>
</s:Envelope>"#;
        assert_eq!(parse_first_profile_token(xml).as_deref(), Some("profile_1"));
    }

    #[test]
    fn ptz_move_and_stop_envelopes_include_profile_token() {
        let move_xml = build_continuous_move_envelope(Some("admin"), Some("secret"), "profile_1", -0.4, 0.0);
        assert!(move_xml.contains("<tptz:ProfileToken>profile_1</tptz:ProfileToken>"));
        assert!(move_xml.contains("x=\"-0.400\""));

        let stop_xml = build_stop_envelope(Some("admin"), Some("secret"), "profile_1");
        assert!(stop_xml.contains("<tptz:Stop"));
        assert!(stop_xml.contains("<tptz:ProfileToken>profile_1</tptz:ProfileToken>"));
    }
}
