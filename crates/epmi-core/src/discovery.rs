use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::net::{IpAddr, SocketAddr, ToSocketAddrs, UdpSocket};
use std::process::Command;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::cloud::match_printer_to_cloud_series;
use crate::snmp::{build_snmp_get_raw_oid, parse_snmp_response};

/// OID `hrDeviceDescr.1` (`1.3.6.1.2.1.25.3.2.1.3.1`) in BER encoding.
const OID_HR_DEVICE_DESCR_BER: [u8; 11] = [
    0x2B, 0x06, 0x01, 0x02, 0x01, 0x19, 0x03, 0x02, 0x01, 0x03, 0x01,
];

/// OID `sysDescr.0` (`1.3.6.1.2.1.1.1.0`) in BER encoding.
const OID_SYS_DESCR_BER: [u8; 8] = [0x2B, 0x06, 0x01, 0x02, 0x01, 0x01, 0x01, 0x00];

/// How an Epson printer was discovered.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum DiscoverySource {
    /// Installed in local CUPS system (`queue_name`).
    CupsQueue(String),
    /// Found on the local network via SNMPv1.
    NetworkSnmp,
    /// Verified both in CUPS and live via SNMPv1 on the network.
    CupsAndNetwork(String),
    /// Manually entered host/IP verified as an Epson printer.
    Manual,
}

impl DiscoverySource {
    pub fn badge_de(&self) -> String {
        match self {
            Self::CupsQueue(q) => format!("CUPS ({q})"),
            Self::NetworkSnmp => String::from("Netzwerk (SNMP)"),
            Self::CupsAndNetwork(q) => format!("CUPS ({q}) & Netzwerk"),
            Self::Manual => String::from("Manuell"),
        }
    }
}

/// A discovered Epson printer (from CUPS and/or local network SNMP discovery).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiscoveredEpsonPrinter {
    /// Hostname or IPv4 address (e.g. `"10.10.10.124"`).
    pub host: String,
    /// Detected printer model name (e.g. `"EPSON SC-P900 Series"`).
    pub model_name: String,
    /// Discovery origin (CUPS queue, Network SNMP, or both).
    pub source: DiscoverySource,
    /// Automatically matched Cloud Catalog series code (e.g. `Some("scp9xx")`).
    pub cloud_series_code: Option<String>,
    /// Whether the printer responded to live SNMP queries during discovery.
    pub online: bool,
}

impl DiscoveredEpsonPrinter {
    /// Formatted label for UI dropdowns (e.g. `"🟢 EPSON SC-P900 Series — 10.10.10.124 [Netzwerk]"`).
    pub fn display_label(&self) -> String {
        let dot = if self.online { "🟢" } else { "⚪" };
        format!(
            "{dot} {} — {} [{}]",
            self.model_name,
            self.host,
            self.source.badge_de()
        )
    }
}

/// Returns `true` if a model description, IEEE-1284 device ID, URI, or PPD text clearly belongs to an Epson printer.
pub fn is_epson_printer_identifier(text: &str) -> bool {
    let upper = text.to_ascii_uppercase();
    upper.contains("EPSON")
        || upper.contains("SURECOLOR")
        || upper.contains("SC-P")
        || upper.contains("SC-T")
        || upper.contains("SC-F")
        || upper.contains("SC-S")
        || upper.contains("ESCPR")
}

fn ber_tlv_simple(tag: u8, val: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(2 + val.len());
    out.push(tag);
    out.push(val.len() as u8);
    out.extend_from_slice(val);
    out
}

fn build_snmp_get_oid(oid_ber: &[u8], req_id: u8) -> Vec<u8> {
    let oid_tlv = ber_tlv_simple(0x06, oid_ber);
    let null_tlv = ber_tlv_simple(0x05, &[]);
    let mut vb_inner = Vec::with_capacity(oid_tlv.len() + null_tlv.len());
    vb_inner.extend_from_slice(&oid_tlv);
    vb_inner.extend_from_slice(&null_tlv);
    let varbind = ber_tlv_simple(0x30, &vb_inner);
    let varbind_list = ber_tlv_simple(0x30, &varbind);

    let req_id_tlv = ber_tlv_simple(0x02, &[req_id & 0x7F]);
    let zero_tlv = ber_tlv_simple(0x02, &[0x00]);
    let mut pdu_inner =
        Vec::with_capacity(req_id_tlv.len() + zero_tlv.len() * 2 + varbind_list.len());
    pdu_inner.extend_from_slice(&req_id_tlv);
    pdu_inner.extend_from_slice(&zero_tlv);
    pdu_inner.extend_from_slice(&zero_tlv);
    pdu_inner.extend_from_slice(&varbind_list);
    let pdu = ber_tlv_simple(0xA0, &pdu_inner);

    let ver_tlv = ber_tlv_simple(0x02, &[0x00]);
    let comm_tlv = ber_tlv_simple(0x04, b"public");
    let mut msg_inner = Vec::with_capacity(ver_tlv.len() + comm_tlv.len() + pdu.len());
    msg_inner.extend_from_slice(&ver_tlv);
    msg_inner.extend_from_slice(&comm_tlv);
    msg_inner.extend_from_slice(&pdu);
    ber_tlv_simple(0x30, &msg_inner)
}

/// Extract the `MDL:...;` field from an IEEE-1284 device ID reply if present.
pub fn parse_ieee1284_model(raw: &[u8]) -> Option<String> {
    let text = String::from_utf8_lossy(raw);
    for part in text.split(';') {
        let trimmed = part.trim();
        for prefix in ["MDL:", "MODEL:"] {
            if let Some( rest ) = trimmed.strip_prefix(prefix) {
                let clean = rest.trim();
                if !clean.is_empty() {
                    return if clean.to_ascii_uppercase().starts_with("EPSON") {
                        Some(clean.to_string())
                    } else {
                        Some(format!("EPSON {clean}"))
                    };
                }
            }
        }
    }
    None
}

/// Probe a single host/IP via SNMPv1 (UDP 161) to determine if it is an Epson printer
/// and return its exact model name (e.g. `"EPSON SC-P900 Series"`).
pub fn probe_epson_printer_snmp(host: &str, timeout: Duration) -> Option<String> {
    let trimmed = host.trim();
    if trimmed.is_empty() {
        return None;
    }
    let target_str = format!("{trimmed}:161");
    let addr: SocketAddr = target_str.to_socket_addrs().ok()?.next()?;
    let bind_addr = if addr.is_ipv4() {
        "0.0.0.0:0"
    } else {
        "[::]:0"
    };
    let sock = UdpSocket::bind(bind_addr).ok()?;
    let _ = sock.set_read_timeout(Some(timeout));
    let _ = sock.set_write_timeout(Some(timeout));

    // 1. Try hrDeviceDescr.1 (returns e.g. "EPSON SC-P900 Series")
    let pkt_hr = build_snmp_get_oid(&OID_HR_DEVICE_DESCR_BER, 11);
    let _ = sock.send_to(&pkt_hr, addr);
    let mut buf = [0u8; 2048];
    if let Ok((n, _)) = sock.recv_from(&mut buf)
        && let Ok(val) = parse_snmp_response(&buf[..n])
    {
        let descr = String::from_utf8_lossy(&val).trim().trim_end_matches('\0').to_string();
        if is_epson_printer_identifier(&descr) {
            return Some(descr);
        }
    }

    // 2. Try Epson IEEE-1284 Device ID command ("di\x01\x00\x01")
    let pkt_di = build_snmp_get_raw_oid(&[0x64, 0x69, 0x01, 0x00, 0x01], "public", 12);
    let _ = sock.send_to(&pkt_di, addr);
    if let Ok((n, _)) = sock.recv_from(&mut buf)
        && let Ok(val) = parse_snmp_response(&buf[..n])
        && let Some(mdl) = parse_ieee1284_model(&val)
        && is_epson_printer_identifier(&mdl)
    {
        return Some(mdl);
    }

    // 3. Try sysDescr.0
    let pkt_sys = build_snmp_get_oid(&OID_SYS_DESCR_BER, 13);
    let _ = sock.send_to(&pkt_sys, addr);
    if let Ok((n, _)) = sock.recv_from(&mut buf)
        && let Ok(val) = parse_snmp_response(&buf[..n])
    {
        let descr = String::from_utf8_lossy(&val).trim().trim_end_matches('\0').to_string();
        if is_epson_printer_identifier(&descr) {
            return Some(descr);
        }
    }

    None
}

/// Extract a network host or IPv4 address from a CUPS device URI (`socket://10.10.10.124:9100`, `ipp://...`, `lpd://...`).
pub fn extract_host_from_cups_uri(uri: &str) -> Option<String> {
    let trimmed = uri.trim();
    for prefix in [
        "socket://",
        "ipp://",
        "ipps://",
        "lpd://",
        "http://",
        "https://",
        " bjnp://",
    ] {
        if let Some(rest) = trimmed.strip_prefix(prefix) {
            let authority = rest.split('/').next().unwrap_or("");
            let host_port = authority.rsplit('@').next().unwrap_or(authority);
            let host = host_port.split(':').next().unwrap_or("").trim();
            if !host.is_empty() && host != "localhost" && host != "127.0.0.1" {
                return Some(host.to_string());
            }
        }
    }
    None
}

fn read_ppd_model_name(queue_name: &str) -> Option<String> {
    let ppd_path = format!("/etc/cups/ppd/{queue_name}.ppd");
    let text = fs::read_to_string(ppd_path).ok()?;
    let mut manufacturer = String::new();
    let mut model = String::new();
    for line in text.lines() {
        let t = line.trim();
        if let Some(rest) = t.strip_prefix("*Manufacturer:") {
            manufacturer = rest.trim().trim_matches('"').to_string();
        } else if let Some(rest) = t.strip_prefix("*NickName:") {
            model = rest.trim().trim_matches('"').to_string();
        } else if model.is_empty()
            && let Some(rest) = t.strip_prefix("*ModelName:")
        {
            model = rest.trim().trim_matches('"').to_string();
        }
    }
    let combined = format!("{manufacturer} {model}").trim().to_string();
    if is_epson_printer_identifier(&combined) {
        Some(if !model.is_empty() { model } else { combined })
    } else {
        None
    }
}

/// Discover Epson printers configured in local CUPS queues (`lpstat -v`).
pub fn discover_cups_epson_printers() -> Vec<(String, String, String)> {
    let mut results = Vec::new();
    let Ok(out) = Command::new("lpstat")
        .args(["-v"])
        .env("LC_ALL", "C")
        .env("LANG", "C")
        .output()
    else {
        return results;
    };
    let stdout = String::from_utf8_lossy(&out.stdout);
    for line in stdout.lines() {
        let trimmed = line.trim();
        let Some(rest) = trimmed.strip_prefix("device for ") else {
            continue;
        };
        let Some((queue_raw, uri_raw)) = rest.split_once(':') else {
            continue;
        };
        let queue = queue_raw.trim();
        let uri = uri_raw.trim();
        let ppd_model = read_ppd_model_name(queue);
        let is_epson = ppd_model.is_some()
            || is_epson_printer_identifier(queue)
            || is_epson_printer_identifier(uri);
        if !is_epson {
            continue;
        }
        if let Some(host) = extract_host_from_cups_uri(uri) {
            let model = ppd_model.unwrap_or_else(|| queue.replace('_', " "));
            results.push((queue.to_string(), host, model));
        }
    }
    results
}

/// Read active IPv4 neighbor addresses from `/proc/net/arp` to speed up unicast SNMP discovery on the LAN.
fn read_arp_neighbor_ips() -> Vec<String> {
    let mut ips = Vec::new();
    let Ok(text) = fs::read_to_string("/proc/net/arp") else {
        return ips;
    };
    for line in text.lines().skip(1) {
        let mut parts = line.split_whitespace();
        let Some(ip_str) = parts.next() else {
            continue;
        };
        let _hw_type = parts.next();
        let flags = parts.next().unwrap_or("0x0");
        // Skip incomplete ARP entries (flags == 0x0)
        if flags == "0x0" {
            continue;
        }
        if ip_str.parse::<IpAddr>().is_ok() {
            ips.push(ip_str.to_string());
        }
    }
    ips
}

/// Discover all Epson printers from both installed CUPS queues and the local network via SNMPv1.
/// Non-Epson devices are strictly filtered out.
pub fn discover_epson_printers(extra_candidate_hosts: &[&str]) -> Vec<DiscoveredEpsonPrinter> {
    let mut by_host: BTreeMap<String, DiscoveredEpsonPrinter> = BTreeMap::new();
    let mut candidate_ips: HashSet<String> = HashSet::new();

    // 1. Collect CUPS Epson printers
    for (queue, host, fallback_model) in discover_cups_epson_printers() {
        candidate_ips.insert(host.clone());
        let cloud_code = match_printer_to_cloud_series(&fallback_model).map(|m| m.code.to_string());
        by_host.insert(
            host.clone(),
            DiscoveredEpsonPrinter {
                host,
                model_name: fallback_model,
                source: DiscoverySource::CupsQueue(queue),
                cloud_series_code: cloud_code,
                online: false,
            },
        );
    }

    // 2. Collect ARP neighbors & explicit candidate hosts
    for ip in read_arp_neighbor_ips() {
        candidate_ips.insert(ip);
    }
    for &h in extra_candidate_hosts {
        let trimmed = h.trim();
        if !trimmed.is_empty() {
            candidate_ips.insert(trimmed.to_string());
        }
    }

    // 3. Fast multi-target UDP SNMPv1 sweep (broadcast + unicast to all candidate IPs on port 161)
    if let Ok(sock) = UdpSocket::bind("0.0.0.0:0") {
        let _ = sock.set_broadcast(true);
        let _ = sock.set_write_timeout(Some(Duration::from_millis(100)));
        let pkt_hr = build_snmp_get_oid(&OID_HR_DEVICE_DESCR_BER, 21);
        let pkt_di = build_snmp_get_raw_oid(&[0x64, 0x69, 0x01, 0x00, 0x01], "public", 22);

        // Send to broadcast
        let _ = sock.send_to(&pkt_hr, "255.255.255.255:161");

        // Send unicast packets to all candidate IPs
        for host in &candidate_ips {
            let target = format!("{host}:161");
            if let Ok(mut addrs) = target.to_socket_addrs()
                && let Some(addr) = addrs.next()
            {
                let _ = sock.send_to(&pkt_hr, addr);
                let _ = sock.send_to(&pkt_di, addr);
            }
        }

        // Collect replies within a 350 ms window
        let deadline = Instant::now() + Duration::from_millis(350);
        let mut buf = [0u8; 2048];
        while Instant::now() < deadline {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                break;
            }
            let _ = sock.set_read_timeout(Some(remaining.min(Duration::from_millis(120))));
            let Ok((n, src_addr)) = sock.recv_from(&mut buf) else {
                break;
            };
            let ip_str = src_addr.ip().to_string();
            let Ok(val) = parse_snmp_response(&buf[..n]) else {
                continue;
            };

            let model_opt = if let Some(mdl) = parse_ieee1284_model(&val) {
                Some(mdl)
            } else {
                let s = String::from_utf8_lossy(&val)
                    .trim()
                    .trim_end_matches('\0')
                    .to_string();
                if is_epson_printer_identifier(&s) {
                    Some(s)
                } else {
                    None
                }
            };

            let Some(model_name) = model_opt else {
                continue;
            };
            if !is_epson_printer_identifier(&model_name) {
                continue;
            }

            let cloud_code = match_printer_to_cloud_series(&model_name).map(|m| m.code.to_string());
            if let Some(existing) = by_host.get_mut(&ip_str) {
                existing.model_name = model_name;
                existing.online = true;
                if existing.cloud_series_code.is_none() {
                    existing.cloud_series_code = cloud_code;
                }
                if let DiscoverySource::CupsQueue(q) = &existing.source {
                    existing.source = DiscoverySource::CupsAndNetwork(q.clone());
                }
            } else {
                by_host.insert(
                    ip_str.clone(),
                    DiscoveredEpsonPrinter {
                        host: ip_str,
                        model_name,
                        source: DiscoverySource::NetworkSnmp,
                        cloud_series_code: cloud_code,
                        online: true,
                    },
                );
            }
        }
    }

    let mut list: Vec<DiscoveredEpsonPrinter> = by_host.into_values().collect();
    // Sort online printers first, then by model name
    list.sort_by(|a, b| {
        b.online
            .cmp(&a.online)
            .then_with(|| a.model_name.cmp(&b.model_name))
            .then_with(|| a.host.cmp(&b.host))
    });
    list
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_epson_printer_identifier() {
        assert!(is_epson_printer_identifier("EPSON SC-P900 Series"));
        assert!(is_epson_printer_identifier("Epson SureColor P700"));
        assert!(!is_epson_printer_identifier("HP LaserJet Pro"));
        assert!(!is_epson_printer_identifier("Canon PRO-1000"));
    }

    #[test]
    fn test_extract_host_from_cups_uri() {
        assert_eq!(
            extract_host_from_cups_uri("socket://10.10.10.124:9100").as_deref(),
            Some("10.10.10.124")
        );
        assert_eq!(
            extract_host_from_cups_uri("ipp://192.168.1.50:631/ipp/print").as_deref(),
            Some("192.168.1.50")
        );
        assert_eq!(extract_host_from_cups_uri("usb://EPSON/SC-P900"), None);
    }

    #[test]
    fn test_parse_ieee1284_model() {
        let sample = b"\x00@BDC PS\r\ndi:MFG:EPSON;CMD:ESCPL2,BDC,D4,D4PX;MDL:SC-P900 Series;CLS:PRINTER;";
        assert_eq!(
            parse_ieee1284_model(sample).as_deref(),
            Some("EPSON SC-P900 Series")
        );
    }
}
