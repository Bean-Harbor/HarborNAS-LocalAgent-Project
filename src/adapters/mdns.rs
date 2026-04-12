//! mDNS discovery adapter boundary.

use std::net::Ipv4Addr;
use std::process::Command;
use std::time::Duration;

use crate::runtime::discovery::{DiscoveryCandidate, DiscoveryRequest};
use crate::runtime::discovery::{DiscoveryCandidateStatus, DiscoveryProtocol};

pub const ADAPTER_NAME: &str = "mdns";

pub trait MdnsDiscoveryAdapter: Send + Sync {
    fn discover(&self, request: &DiscoveryRequest) -> Result<Vec<DiscoveryCandidate>, String>;
}

/// mDNS discovery implementation backed by `avahi-browse` (Debian-friendly).
///
/// We intentionally keep this implementation thin and command-backed so we don't
/// pull a full mDNS stack into the Rust runtime. This is enough for Debian 13
/// real-usage where `avahi-daemon` is already part of the onboarding story.
#[derive(Debug, Clone)]
pub struct AvahiMdnsAdapter {
    browse_bin: String,
    service_type: String,
}

impl AvahiMdnsAdapter {
    pub fn new(browse_bin: impl Into<String>, _timeout: Duration, service_type: impl Into<String>) -> Self {
        Self {
            browse_bin: browse_bin.into(),
            service_type: service_type.into(),
        }
    }

    fn parse_avahi_line(line: &str) -> Option<ParsedAvahiService> {
        // Example (`-p` output, `;` delimited):
        // =;eth0;IPv4;My Cam;_rtsp._tcp;local;cam.local;192.168.1.20;554;"txt=foo"
        let trimmed = line.trim();
        if trimmed.is_empty() || !trimmed.starts_with('=') {
            return None;
        }
        let mut parts = trimmed.split(';').collect::<Vec<_>>();
        if parts.len() < 9 {
            return None;
        }
        // parts[0] == "=" or "=+"
        let _iface = parts.get(1).copied().unwrap_or_default();
        let proto = parts.get(2).copied().unwrap_or_default();
        if proto != "IPv4" {
            return None;
        }
        let name = parts.get(3).copied().unwrap_or_default().trim().to_string();
        let host = parts.get(6).copied().unwrap_or_default().trim().to_string();
        let ip = parts.get(7).copied().unwrap_or_default().trim().to_string();
        let port = parts
            .get(8)
            .and_then(|value| value.trim().parse::<u16>().ok())
            .unwrap_or(0);
        if ip.parse::<Ipv4Addr>().is_err() || port == 0 {
            return None;
        }

        // Remaining fields are TXT; best-effort extract a couple of hints.
        let txt = parts.drain(9..).collect::<Vec<_>>().join(";");
        let vendor = extract_txt_value(&txt, "manufacturer")
            .or_else(|| extract_txt_value(&txt, "vendor"));
        let model = extract_txt_value(&txt, "model");

        Some(ParsedAvahiService {
            name: if name.is_empty() { None } else { Some(name) },
            host: if host.is_empty() { None } else { Some(host) },
            ip,
            port,
            vendor,
            model,
        })
    }
}

impl Default for AvahiMdnsAdapter {
    fn default() -> Self {
        Self::new("avahi-browse", Duration::from_secs(4), "_rtsp._tcp")
    }
}

impl MdnsDiscoveryAdapter for AvahiMdnsAdapter {
    fn discover(&self, request: &DiscoveryRequest) -> Result<Vec<DiscoveryCandidate>, String> {
        let (network, prefix) = parse_ipv4_cidr(&request.network_cidr)?;

        let output = Command::new(&self.browse_bin)
            .arg("-rtp")
            .arg(&self.service_type)
            .arg("local")
            .output()
            .map_err(|e| format!("mDNS discovery failed to launch {}: {e}", self.browse_bin))?;

        let text = String::from_utf8_lossy(&output.stdout);
        let mut candidates = Vec::new();
        for line in text.lines() {
            let Some(parsed) = Self::parse_avahi_line(line) else {
                continue;
            };
            let Ok(ipv4) = parsed.ip.parse::<Ipv4Addr>() else {
                continue;
            };
            if !ipv4_in_cidr(ipv4, network, prefix) {
                continue;
            }

            let candidate_id = parsed
                .host
                .clone()
                .unwrap_or_else(|| format!("mdns-{}", parsed.ip.replace('.', "-")));
            let display_name = parsed.name.clone().or_else(|| parsed.host.clone());

            candidates.push(DiscoveryCandidate {
                candidate_id,
                protocol: DiscoveryProtocol::Mdns,
                name: display_name,
                ip_address: parsed.ip,
                port: Some(parsed.port),
                vendor: parsed.vendor,
                model: parsed.model,
                rtsp_paths: Vec::new(),
                status: DiscoveryCandidateStatus::Discovered,
            });
        }

        Ok(candidates)
    }
}

#[derive(Debug, Clone)]
struct ParsedAvahiService {
    name: Option<String>,
    host: Option<String>,
    ip: String,
    port: u16,
    vendor: Option<String>,
    model: Option<String>,
}

fn extract_txt_value(txt: &str, key: &str) -> Option<String> {
    let needle = format!("{key}=");
    let mut best: Option<String> = None;
    for chunk in txt.split('"').flat_map(|item| item.split_whitespace()) {
        let chunk = chunk.trim_matches(|ch: char| ch == ';' || ch == '"' || ch == ',');
        if let Some(rest) = chunk.strip_prefix(&needle) {
            let value = rest.trim().trim_matches('"');
            if !value.is_empty() {
                best = Some(value.to_string());
                break;
            }
        }
    }
    best
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
    let mask = if prefix == 0 { 0 } else { u32::MAX << (32 - prefix) };
    (u32::from(ip) & mask) == (u32::from(network) & mask)
}

#[cfg(test)]
mod tests {
    use super::AvahiMdnsAdapter;

    #[test]
    fn parses_avahi_browse_ipv4_line() {
        let line = r#"=;eth0;IPv4;Living Room Cam;_rtsp._tcp;local;cam.local;192.168.1.20;554;"model=C6c" "manufacturer=EZVIZ""#;
        let parsed = AvahiMdnsAdapter::parse_avahi_line(line).expect("parsed");
        assert_eq!(parsed.ip, "192.168.1.20");
        assert_eq!(parsed.port, 554);
        assert_eq!(parsed.name.as_deref(), Some("Living Room Cam"));
        assert_eq!(parsed.vendor.as_deref(), Some("EZVIZ"));
        assert_eq!(parsed.model.as_deref(), Some("C6c"));
    }

    #[test]
    fn ignores_non_ipv4_lines() {
        let line = r#"=;eth0;IPv6;Cam;_rtsp._tcp;local;cam.local;fe80::1;554;"#;
        assert!(AvahiMdnsAdapter::parse_avahi_line(line).is_none());
    }
}
