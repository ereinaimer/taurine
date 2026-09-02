use std::net::{IpAddr, Ipv4Addr, SocketAddr, SocketAddrV4, TcpStream, UdpSocket};
use std::time::Duration;

pub fn resolve(key: &str) -> Option<String> {
    if !key.starts_with("net.") {
        return None;
    }

    let modifier = &key[4..];
    if modifier == "ip" {
        Some(resolve_public_ip())
    } else if modifier == "lip" {
        Some(resolve_local_ip())
    } else if modifier == "online" {
        Some(resolve_online())
    } else {
        None
    }
}

fn resolve_local_ip() -> String {
    if let Some(ip) = routed_local_ipv4() {
        return ip.to_string();
    }
    "[Error: no local IP found]".to_string()
}

fn routed_local_ipv4() -> Option<IpAddr> {
    let socket = UdpSocket::bind(SocketAddr::from((Ipv4Addr::UNSPECIFIED, 0))).ok()?;
    socket
        .connect(SocketAddr::from((Ipv4Addr::new(1, 1, 1, 1), 53)))
        .ok()?;
    let ip = socket.local_addr().ok()?.ip();
    if ip.is_loopback() || ip.is_unspecified() {
        None
    } else {
        Some(ip)
    }
}

fn resolve_online() -> String {
    let addr = SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::new(1, 1, 1, 1), 53));
    let timeout = Duration::from_millis(500);
    if TcpStream::connect_timeout(&addr, timeout).is_ok() {
        "true".to_string()
    } else {
        "false".to_string()
    }
}

fn parse_trace_response(body: &str) -> Option<String> {
    for line in body.lines() {
        if let Some(ip) = line.trim().strip_prefix("ip=") {
            let ip = ip.trim();
            if !ip.is_empty() {
                return Some(ip.to_string());
            }
        }
    }
    None
}

fn parse_plain_ip_response(body: &str) -> Option<String> {
    let ip = body.trim();
    if !ip.is_empty() && !ip.contains('<') && !ip.contains("HTTP") {
        Some(ip.to_string())
    } else {
        None
    }
}

fn resolve_public_ip() -> String {
    let timeout = Duration::from_millis(2000);

    if let Ok(res) = ureq::get("https://1.1.1.1/cdn-cgi/trace")
        .timeout(timeout)
        .call()
        && let Ok(body) = res.into_string()
        && let Some(ip) = parse_trace_response(&body)
    {
        return ip;
    }

    for url in ["https://api.ipify.org", "https://checkip.amazonaws.com"] {
        if let Ok(res) = ureq::get(url).timeout(timeout).call()
            && let Ok(body) = res.into_string()
            && let Some(ip) = parse_plain_ip_response(&body)
        {
            return ip;
        }
    }

    "[Error: public IP unavailable]".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_trace_response() {
        let sample = "fl=123\nh=1.1.1.1\nip=198.51.100.42\nts=12345678\n";
        assert_eq!(
            parse_trace_response(sample),
            Some("198.51.100.42".to_string())
        );

        let empty = "fl=123\nh=1.1.1.1\nts=12345678\n";
        assert_eq!(parse_trace_response(empty), None);
    }

    #[test]
    fn test_parse_plain_ip_response() {
        assert_eq!(
            parse_plain_ip_response(" 203.0.113.19 \n"),
            Some("203.0.113.19".to_string())
        );
        assert_eq!(parse_plain_ip_response("<html>Error</html>"), None);
        assert_eq!(parse_plain_ip_response("HTTP 500"), None);
        assert_eq!(parse_plain_ip_response(""), None);
    }

    #[test]
    fn test_resolve_routing() {
        assert!(resolve("net.ip").is_some());
        assert!(resolve("net.lip").is_some());
        let online = resolve("net.online").unwrap();
        assert!(online == "true" || online == "false");
    }

    #[test]
    fn test_resolve_unknown_modifier() {
        assert_eq!(resolve("net"), None);
        assert_eq!(resolve("net."), None);
        assert_eq!(resolve("net.mac"), None);
        assert_eq!(resolve("net.hostname"), None);
        assert_eq!(resolve("not_net.ip"), None);
    }
}
