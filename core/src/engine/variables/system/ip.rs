use std::net::{IpAddr, Ipv4Addr, SocketAddr, UdpSocket};
use std::time::Duration;

/// Resolves the unified `ip(...)` system variable.
///
/// `raw` is the argument list inside `ip(...)` (`""` when bare),
/// bound as `(type=public)` with `type` in `{public, local}`.
pub fn resolve(raw: &str) -> Option<String> {
    let spec = crate::engine::variables::registry::param_spec("ip")?;
    let bound = crate::engine::variables::parser::bind_call("ip", raw, &spec).ok()?;
    if bound.positional.len() > spec.params.len() {
        return None;
    }
    match bound.named.get("type").map(String::as_str) {
        Some("public") => resolve_public_ip(),
        Some("local") => resolve_local_ip(),
        _ => None,
    }
}

fn resolve_local_ip() -> Option<String> {
    if let Some(ip) = routed_local_ipv4() {
        return Some(ip.to_string());
    }
    tracing::warn!("Failed to resolve local IP address");
    None
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

fn fetch_url(url: &str, timeout: Duration) -> Option<String> {
    ureq::get(url)
        .timeout(timeout)
        .call()
        .ok()?
        .into_string()
        .ok()
}

fn resolve_public_ip_with(fetch: &dyn Fn(&str) -> Option<String>) -> Option<String> {
    if let Some(body) = fetch("https://1.1.1.1/cdn-cgi/trace")
        && let Some(ip) = parse_trace_response(&body)
    {
        return Some(ip);
    }

    for url in ["https://api.ipify.org", "https://checkip.amazonaws.com"] {
        if let Some(body) = fetch(url)
            && let Some(ip) = parse_plain_ip_response(&body)
        {
            return Some(ip);
        }
    }

    tracing::warn!("Failed to resolve public IP address");
    None
}

fn resolve_public_ip() -> Option<String> {
    let timeout = Duration::from_millis(2000);
    resolve_public_ip_with(&|url| fetch_url(url, timeout))
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
    fn ip_unified() {
        assert!(resolve("local").is_some());
        assert!(resolve("type=local").is_some());
        // Public WAN fetch is deferred + network-dependent; stub the transport.
        assert_eq!(
            resolve_public_ip_with(&|_| Some("fl=1\nip=198.51.100.42\n".to_string())),
            Some("198.51.100.42".to_string())
        );
        assert_eq!(
            resolve_public_ip_with(&|url| {
                if url.contains("trace") {
                    Some("<html>busy</html>".to_string())
                } else {
                    Some("203.0.113.19\n".to_string())
                }
            }),
            Some("203.0.113.19".to_string())
        );
        assert_eq!(resolve_public_ip_with(&|_| None), None);
        assert_eq!(resolve("online"), None);
        assert_eq!(resolve("private"), None);
        assert_eq!(resolve("publicip"), None);
    }
}
