use std::net::{IpAddr, Ipv4Addr, SocketAddr, SocketAddrV4};
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpStream, UdpSocket};

const SSDP_MULTICAST: SocketAddrV4 = SocketAddrV4::new(Ipv4Addr::new(239, 255, 255, 250), 1900);
const IGD_SERVICES: [&str; 3] = [
    "urn:schemas-upnp-org:service:WANIPConnection:1",
    "urn:schemas-upnp-org:service:WANIPConnection:2",
    "urn:schemas-upnp-org:service:WANPPPConnection:1",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpnpMappingResult {
    pub external_ip: Ipv4Addr,
    pub tcp_mapped: bool,
    pub udp_mapped: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedHttpUrl {
    pub host: String,
    pub port: u16,
    pub path: String,
}

/// Returns true only when an IPv4 address is globally routable (excludes private, loopback,
/// link-local, unspecified, multicast, and ISP CGNAT 100.64.0.0/10 ranges).
pub fn is_public_routable_ipv4(ip: Ipv4Addr) -> bool {
    let octets = ip.octets();
    let is_cgnat = octets[0] == 100 && (octets[1] & 0b1100_0000) == 64;
    !ip.is_private()
        && !ip.is_loopback()
        && !ip.is_link_local()
        && !ip.is_broadcast()
        && !ip.is_unspecified()
        && !ip.is_multicast()
        && !is_cgnat
        && octets[0] != 0
}

pub fn parse_http_url(url: &str) -> Option<ParsedHttpUrl> {
    let rest = url.trim().strip_prefix("http://")?;
    let (authority, path_part) = match rest.find('/') {
        Some(idx) => (&rest[..idx], &rest[idx..]),
        None => (rest, "/"),
    };
    let (host, port) = match authority.rsplit_once(':') {
        Some((h, p)) => (h.to_string(), p.parse::<u16>().ok()?),
        None => (authority.to_string(), 80),
    };
    if host.is_empty() {
        return None;
    }
    Some(ParsedHttpUrl {
        host,
        port,
        path: if path_part.is_empty() {
            "/".to_string()
        } else {
            path_part.to_string()
        },
    })
}

pub fn parse_ssdp_locations(raw: &str) -> Vec<String> {
    let mut locations = Vec::new();
    for line in raw.lines() {
        let trimmed = line.trim();
        if let Some((name, value)) = trimmed.split_once(':') {
            if name.trim().eq_ignore_ascii_case("location") {
                let loc = value.trim();
                if loc.starts_with("http://") && !locations.contains(&loc.to_string()) {
                    locations.push(loc.to_string());
                }
            }
        }
    }
    locations
}

fn extract_xml_tag_case_insensitive(xml: &str, tag: &str) -> Option<String> {
    let lower = xml.to_ascii_lowercase();
    let open_pattern = format!("<{}>", tag.to_ascii_lowercase());
    let close_pattern = format!("</{}>", tag.to_ascii_lowercase());
    let start = lower.find(&open_pattern)? + open_pattern.len();
    let rest_lower = &lower[start..];
    let rel_end = rest_lower.find(&close_pattern)?;
    Some(xml[start..start + rel_end].trim().to_string())
}

pub fn resolve_control_url(base_url: &ParsedHttpUrl, control_url: &str) -> Option<ParsedHttpUrl> {
    let trimmed = control_url.trim();
    if trimmed.starts_with("http://") {
        return parse_http_url(trimmed);
    }
    let normalized_path = if trimmed.starts_with('/') {
        trimmed.to_string()
    } else {
        let base_dir = match base_url.path.rfind('/') {
            Some(idx) => &base_url.path[..=idx],
            None => "/",
        };
        format!("{base_dir}{trimmed}")
    };
    Some(ParsedHttpUrl {
        host: base_url.host.clone(),
        port: base_url.port,
        path: normalized_path,
    })
}

pub fn parse_igd_control_url(
    xml: &str,
    location_url: &ParsedHttpUrl,
) -> Option<(String, ParsedHttpUrl)> {
    let base = extract_xml_tag_case_insensitive(xml, "URLBase")
        .and_then(|u| parse_http_url(&u))
        .unwrap_or_else(|| location_url.clone());

    let lower = xml.to_ascii_lowercase();
    let mut cursor = 0usize;
    while let Some(rel_start) = lower[cursor..].find("<service>") {
        let block_start = cursor + rel_start;
        let Some(rel_end) = lower[block_start..].find("</service>") else {
            break;
        };
        let block_end = block_start + rel_end + "</service>".len();
        let block = &xml[block_start..block_end];

        if let Some(svc_type) = extract_xml_tag_case_insensitive(block, "serviceType") {
            if IGD_SERVICES
                .iter()
                .any(|s| s.eq_ignore_ascii_case(svc_type.trim()))
            {
                if let Some(ctrl_raw) = extract_xml_tag_case_insensitive(block, "controlURL") {
                    if let Some(resolved) = resolve_control_url(&base, &ctrl_raw) {
                        return Some((svc_type.trim().to_string(), resolved));
                    }
                }
            }
        }
        cursor = block_end;
    }
    None
}

pub fn parse_external_ip_xml(xml: &str) -> Option<Ipv4Addr> {
    let raw = extract_xml_tag_case_insensitive(xml, "NewExternalIPAddress")?;
    raw.trim().parse::<Ipv4Addr>().ok()
}

pub fn format_add_port_mapping_soap(
    service_type: &str,
    external_port: u16,
    internal_port: u16,
    internal_client: Ipv4Addr,
    protocol: &str,
    lease_secs: u32,
    description: &str,
) -> String {
    format!(
        "<?xml version=\"1.0\"?>\r\n\
         <s:Envelope xmlns:s=\"http://schemas.xmlsoap.org/soap/envelope/\" \
         s:encodingStyle=\"http://schemas.xmlsoap.org/soap/encoding/\">\r\n\
           <s:Body>\r\n\
             <u:AddPortMapping xmlns:u=\"{service_type}\">\r\n\
               <NewRemoteHost></NewRemoteHost>\r\n\
               <NewExternalPort>{external_port}</NewExternalPort>\r\n\
               <NewProtocol>{protocol}</NewProtocol>\r\n\
               <NewInternalPort>{internal_port}</NewInternalPort>\r\n\
               <NewInternalClient>{internal_client}</NewInternalClient>\r\n\
               <NewEnabled>1</NewEnabled>\r\n\
               <NewPortMappingDescription>{description}</NewPortMappingDescription>\r\n\
               <NewLeaseDuration>{lease_secs}</NewLeaseDuration>\r\n\
             </u:AddPortMapping>\r\n\
           </s:Body>\r\n\
         </s:Envelope>"
    )
}

async fn http_exchange(
    target: &ParsedHttpUrl,
    request_bytes: &[u8],
) -> Option<(Ipv4Addr, String, bool)> {
    let ip: Ipv4Addr = target.host.parse().ok()?;
    let sock = SocketAddr::V4(SocketAddrV4::new(ip, target.port));
    let mut stream = tokio::time::timeout(Duration::from_millis(1200), TcpStream::connect(sock))
        .await
        .ok()?
        .ok()?;
    let local_ip = match stream.local_addr().ok()?.ip() {
        IpAddr::V4(v4) => v4,
        IpAddr::V6(_) => return None,
    };

    tokio::time::timeout(Duration::from_millis(1200), stream.write_all(request_bytes))
        .await
        .ok()?
        .ok()?;

    let mut resp = Vec::new();
    let mut chunk = [0u8; 4096];
    let deadline = tokio::time::Instant::now() + Duration::from_millis(1400);
    while resp.len() < 256 * 1024 {
        let rem = deadline.saturating_duration_since(tokio::time::Instant::now());
        if rem.is_zero() {
            break;
        }
        match tokio::time::timeout(rem, stream.read(&mut chunk)).await {
            Ok(Ok(0)) => break,
            Ok(Ok(n)) => resp.extend_from_slice(&chunk[..n]),
            _ => break,
        }
    }
    let text = String::from_utf8_lossy(&resp).into_owned();
    let status_ok = text
        .lines()
        .next()
        .map(|l| l.contains(" 200 "))
        .unwrap_or(false);
    let body = text
        .split_once("\r\n\r\n")
        .map(|(_, b)| b.to_string())
        .or_else(|| text.split_once("\n\n").map(|(_, b)| b.to_string()))
        .unwrap_or(text);
    Some((local_ip, body, status_ok))
}

async fn soap_call(
    control: &ParsedHttpUrl,
    service_type: &str,
    action: &str,
    body_xml: &str,
) -> Option<(Ipv4Addr, String, bool)> {
    let req = format!(
        "POST {} HTTP/1.0\r\n\
         Host: {}:{}\r\n\
         Content-Type: text/xml; charset=\"utf-8\"\r\n\
         SOAPACTION: \"{}#{}\"\r\n\
         Content-Length: {}\r\n\
         Connection: close\r\n\r\n{}",
        control.path,
        control.host,
        control.port,
        service_type,
        action,
        body_xml.len(),
        body_xml
    );
    http_exchange(control, req.as_bytes()).await
}

async fn discover_ssdp_locations() -> Vec<String> {
    let Ok(sock) = UdpSocket::bind(SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, 0)).await else {
        return Vec::new();
    };
    for st in [
        "urn:schemas-upnp-org:device:InternetGatewayDevice:1",
        "urn:schemas-upnp-org:device:InternetGatewayDevice:2",
        "urn:schemas-upnp-org:service:WANIPConnection:1",
        "urn:schemas-upnp-org:service:WANPPPConnection:1",
    ] {
        let query = format!(
            "M-SEARCH * HTTP/1.1\r\n\
             HOST: 239.255.255.250:1900\r\n\
             MAN: \"ssdp:discover\"\r\n\
             MX: 1\r\n\
             ST: {st}\r\n\r\n"
        );
        let _ = sock.send_to(query.as_bytes(), SSDP_MULTICAST).await;
    }

    let mut locations = Vec::new();
    let mut buf = [0u8; 4096];
    let deadline = tokio::time::Instant::now() + Duration::from_millis(850);
    while locations.len() < 4 {
        let rem = deadline.saturating_duration_since(tokio::time::Instant::now());
        if rem.is_zero() {
            break;
        }
        match tokio::time::timeout(rem, sock.recv_from(&mut buf)).await {
            Ok(Ok((n, _))) => {
                let raw = String::from_utf8_lossy(&buf[..n]);
                for loc in parse_ssdp_locations(&raw) {
                    if !locations.contains(&loc) {
                        locations.push(loc);
                    }
                }
            }
            _ => break,
        }
    }
    locations
}

/// Discovers a local UPnP IGD router without any central server, forwards TCP/UDP mesh ports,
/// and returns the router's globally routable IPv4 when available.
pub async fn discover_and_map_ports(tcp_port: u16, udp_port: u16) -> Option<UpnpMappingResult> {
    let locations = discover_ssdp_locations().await;
    for loc_str in locations {
        let Some(loc_url) = parse_http_url(&loc_str) else {
            continue;
        };
        let get_req = format!(
            "GET {} HTTP/1.0\r\nHost: {}:{}\r\nConnection: close\r\n\r\n",
            loc_url.path, loc_url.host, loc_url.port
        );
        let Some((_, desc_xml, ok)) = http_exchange(&loc_url, get_req.as_bytes()).await else {
            continue;
        };
        if !ok {
            continue;
        }
        let Some((service_type, control_url)) = parse_igd_control_url(&desc_xml, &loc_url) else {
            continue;
        };

        let ext_soap = format!(
            "<?xml version=\"1.0\"?>\r\n\
             <s:Envelope xmlns:s=\"http://schemas.xmlsoap.org/soap/envelope/\" \
             s:encodingStyle=\"http://schemas.xmlsoap.org/soap/encoding/\">\r\n\
               <s:Body>\r\n\
                 <u:GetExternalIPAddress xmlns:u=\"{service_type}\"/>\r\n\
               </s:Body>\r\n\
             </s:Envelope>"
        );
        let Some((local_ip, ext_resp, ext_ok)) =
            soap_call(&control_url, &service_type, "GetExternalIPAddress", &ext_soap).await
        else {
            continue;
        };
        if !ext_ok {
            continue;
        }
        let Some(external_ip) = parse_external_ip_xml(&ext_resp) else {
            continue;
        };
        if !is_public_routable_ipv4(external_ip) {
            continue;
        }

        let tcp_body = format_add_port_mapping_soap(
            &service_type,
            tcp_port,
            tcp_port,
            local_ip,
            "TCP",
            3600,
            "Cymesh Mesh TCP",
        );
        let tcp_mapped = soap_call(&control_url, &service_type, "AddPortMapping", &tcp_body)
            .await
            .map(|(_, _, ok)| ok)
            .unwrap_or(false);

        let udp_body = format_add_port_mapping_soap(
            &service_type,
            udp_port,
            udp_port,
            local_ip,
            "UDP",
            3600,
            "Cymesh Mesh UDP",
        );
        let udp_mapped = soap_call(&control_url, &service_type, "AddPortMapping", &udp_body)
            .await
            .map(|(_, _, ok)| ok)
            .unwrap_or(false);

        if tcp_mapped || udp_mapped {
            return Some(UpnpMappingResult {
                external_ip,
                tcp_mapped,
                udp_mapped,
            });
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ssdp_and_url_parsing() {
        let ssdp = "HTTP/1.1 200 OK\r\nCACHE-CONTROL: max-age=120\r\nLocation: http://192.168.1.1:1900/igd.xml\r\nST: urn:schemas-upnp-org:device:InternetGatewayDevice:1\r\n\r\n";
        let locs = parse_ssdp_locations(ssdp);
        assert_eq!(locs, vec!["http://192.168.1.1:1900/igd.xml"]);

        let parsed = parse_http_url(&locs[0]).expect("valid http url");
        assert_eq!(parsed.host, "192.168.1.1");
        assert_eq!(parsed.port, 1900);
        assert_eq!(parsed.path, "/igd.xml");
    }

    #[test]
    fn test_igd_xml_control_url_and_external_ip() {
        let base = parse_http_url("http://192.168.1.1:5000/rootDesc.xml").unwrap();
        let xml = r#"
        <root>
          <URLBase>http://192.168.1.1:5000</URLBase>
          <device>
            <serviceList>
              <service>
                <serviceType>urn:schemas-upnp-org:service:Layer3Forwarding:1</serviceType>
                <controlURL>/ctl/L3F</controlURL>
              </service>
              <service>
                <serviceType>urn:schemas-upnp-org:service:WANIPConnection:1</serviceType>
                <controlURL>/ctl/IPConn</controlURL>
              </service>
            </serviceList>
          </device>
        </root>"#;

        let (svc, ctrl) = parse_igd_control_url(xml, &base).expect("must find WANIPConnection");
        assert_eq!(svc, "urn:schemas-upnp-org:service:WANIPConnection:1");
        assert_eq!(ctrl.host, "192.168.1.1");
        assert_eq!(ctrl.port, 5000);
        assert_eq!(ctrl.path, "/ctl/IPConn");

        let soap_resp = "<s:Envelope><s:Body><u:GetExternalIPAddressResponse><NewExternalIPAddress>203.0.113.44</NewExternalIPAddress></u:GetExternalIPAddressResponse></s:Body></s:Envelope>";
        assert_eq!(
            parse_external_ip_xml(soap_resp),
            Some(Ipv4Addr::new(203, 0, 113, 44))
        );
    }

    #[test]
    fn test_public_routable_ipv4_filters_private_and_cgnat() {
        assert!(!is_public_routable_ipv4(Ipv4Addr::new(192, 168, 1, 10)));
        assert!(!is_public_routable_ipv4(Ipv4Addr::new(10, 0, 0, 5)));
        assert!(!is_public_routable_ipv4(Ipv4Addr::new(100, 64, 12, 8)));
        assert!(!is_public_routable_ipv4(Ipv4Addr::new(100, 127, 255, 1)));
        assert!(!is_public_routable_ipv4(Ipv4Addr::new(127, 0, 0, 1)));
        assert!(is_public_routable_ipv4(Ipv4Addr::new(1, 1, 1, 1)));
        assert!(is_public_routable_ipv4(Ipv4Addr::new(100, 128, 0, 1)));
    }
}
