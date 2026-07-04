use std::io::{Read, Write};
use std::net::{
    IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, SocketAddrV4, SocketAddrV6, TcpStream, ToSocketAddrs,
    UdpSocket,
};
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
    } else if modifier.starts_with("port(") {
        let rest = modifier.strip_prefix("port(")?;
        let port_str = rest.strip_suffix(')')?;
        resolve_port(crate::engine::variables::system::strip_argument_quotes(
            port_str,
        ))
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

fn resolve_port(port_str: &str) -> Option<String> {
    let port = port_str.trim().parse::<u16>().ok()?;
    let timeout = Duration::from_millis(200);

    let v4_addr = SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::new(127, 0, 0, 1), port));
    if TcpStream::connect_timeout(&v4_addr, timeout).is_ok() {
        return Some("true".to_string());
    }

    let v6_addr = SocketAddr::V6(SocketAddrV6::new(Ipv6Addr::LOCALHOST, port, 0, 0));
    if TcpStream::connect_timeout(&v6_addr, timeout).is_ok() {
        return Some("true".to_string());
    }

    Some("false".to_string())
}

fn resolve_public_ip() -> String {
    let timeout = Duration::from_millis(1500);

    let addrs = [
        SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::new(1, 1, 1, 1), 80)),
        SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::new(1, 0, 0, 1), 80)),
    ];

    for socket_addr in addrs {
        if let Ok(mut stream) = TcpStream::connect_timeout(&socket_addr, timeout) {
            let _ = stream.set_read_timeout(Some(timeout));
            let _ = stream.set_write_timeout(Some(timeout));
            let req = "GET /cdn-cgi/trace HTTP/1.1\r\nHost: 1.1.1.1\r\nUser-Agent: Taurine/1.0\r\nConnection: close\r\n\r\n";
            let mut buf = [0u8; 2048];
            if stream.write_all(req.as_bytes()).is_ok()
                && let Ok(n) = stream.read(&mut buf)
                && n > 0
                && let Ok(response) = String::from_utf8(buf[..n].to_vec())
            {
                for line in response.lines() {
                    if let Some(ip) = line.trim().strip_prefix("ip=") {
                        let ip = ip.trim();
                        if !ip.is_empty() {
                            return ip.to_string();
                        }
                    }
                }
            }
        }
    }

    for (host, path) in [("api.ipify.org", "/"), ("checkip.amazonaws.com", "/")] {
        let addr_str = format!("{}:80", host);
        if let Ok(mut addrs) = addr_str.to_socket_addrs()
            && let Some(socket_addr) = addrs.next()
            && let Ok(mut stream) = TcpStream::connect_timeout(&socket_addr, timeout)
        {
            let _ = stream.set_read_timeout(Some(timeout));
            let _ = stream.set_write_timeout(Some(timeout));
            let req = format!(
                "GET {} HTTP/1.1\r\nHost: {}\r\nUser-Agent: Taurine/1.0\r\nConnection: close\r\n\r\n",
                path, host
            );
            let mut buf = [0u8; 2048];
            if stream.write_all(req.as_bytes()).is_ok()
                && let Ok(n) = stream.read(&mut buf)
                && n > 0
                && let Ok(response) = String::from_utf8(buf[..n].to_vec())
                && let Some(body) = response.split("\r\n\r\n").nth(1)
            {
                let ip = body.trim();
                if !ip.is_empty() && !ip.contains('<') && !ip.contains("HTTP") {
                    return ip.to_string();
                }
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
    fn test_resolve_port() {
        let res = resolve("net.port(9999)").unwrap();
        assert_eq!(res, "false");
        assert_eq!(resolve("net.port(invalid)"), None);
    }

    #[test]
    fn test_resolve_unknown_modifier() {
        assert_eq!(resolve("net"), None);
        assert_eq!(resolve("net.mac"), None);
        assert_eq!(resolve("net.hostname"), None);
    }
}
