use std::net::{IpAddr, Ipv4Addr, SocketAddr, UdpSocket};

/// Resolves the zero-variant `ip` system variable.
///
/// Returns all available local interface IP addresses joined by comma (e.g. `192.168.1.15, 10.0.0.8`).
/// Takes zero arguments/variants.
pub fn resolve(raw: &str) -> Option<String> {
    if !raw.trim().is_empty() {
        return None;
    }

    let mut addrs = Vec::new();

    let networks = sysinfo::Networks::new_with_refreshed_list();
    for (_interface_name, data) in &networks {
        for ip_net in data.ip_networks() {
            let addr = ip_net.addr;
            if !addr.is_loopback() && !addr.is_unspecified() && !addrs.contains(&addr) {
                addrs.push(addr);
            }
        }
    }

    if addrs.is_empty()
        && let Some(ip) = routed_local_ipv4()
    {
        addrs.push(ip);
    }

    if addrs.is_empty() {
        addrs.push(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)));
    }

    let formatted: Vec<String> = addrs.into_iter().map(|ip| ip.to_string()).collect();
    Some(formatted.join(", "))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ip_zero_variant_returns_local_ips() {
        let res = resolve("").expect("ip must resolve synchronously");
        assert!(!res.is_empty());
        // Must reject arguments
        assert_eq!(resolve("public"), None);
        assert_eq!(resolve("local"), None);
    }
}
