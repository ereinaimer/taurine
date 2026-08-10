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

fn resolve_public_ip() -> String {
    let timeout = Duration::from_millis(2000);

    if let Ok(res) = ureq::get("https://1.1.1.1/cdn-cgi/trace")
        .timeout(timeout)
        .call()
        && let Ok(body) = res.into_string()
    {
        for line in body.lines() {
            if let Some(ip) = line.trim().strip_prefix("ip=") {
                let ip = ip.trim();
                if !ip.is_empty() {
                    return ip.to_string();
                }
            }
        }
    }

    for url in ["https://api.ipify.org", "https://checkip.amazonaws.com"] {
        if let Ok(res) = ureq::get(url).timeout(timeout).call()
            && let Ok(body) = res.into_string()
        {
            let ip = body.trim();
            if !ip.is_empty() && !ip.contains('<') && !ip.contains("HTTP") {
                return ip.to_string();
            }
        }
    }

    "[Error: public IP unavailable]".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resolve_ip() {
        let res = resolve("net.ip").unwrap();
        assert!(!res.starts_with("[Error"));
        assert!(res.contains('.'));
    }

    #[test]
    fn test_resolve_lip() {
        let res = resolve("net.lip").unwrap();
        assert!(!res.starts_with("[Error"));
        assert!(res.contains('.'));
    }

    #[test]
    fn test_resolve_online() {
        let res = resolve("net.online").unwrap();
        assert!(res == "true" || res == "false");
    }

    #[test]
    fn test_resolve_unknown_modifier() {
        assert_eq!(resolve("net"), None);
        assert_eq!(resolve("net.mac"), None);
        assert_eq!(resolve("net.hostname"), None);
    }
}
