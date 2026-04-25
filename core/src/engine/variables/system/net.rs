use std::cmp::Ordering;
use std::net::IpAddr;

use network_interface::{Addr, NetworkInterface, NetworkInterfaceConfig};

/// Resolves `net.*` system variables.
pub fn resolve(key: &str) -> Option<String> {
    if !key.starts_with("net.") {
        return None;
    }

    match &key[4..] {
        "hostname" => Some(resolve_hostname()),
        "localip" => Some(resolve_local_ip()),
        "mac" => Some(resolve_mac()),
        _ => None,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct InterfaceSnapshot {
    name: String,
    index: u32,
    internal: bool,
    mac: Option<String>,
    addrs: Vec<IpAddr>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct AddressCandidate {
    interface_name: String,
    interface_index: u32,
    internal: bool,
    ip: IpAddr,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct MacCandidate {
    interface_name: String,
    interface_index: u32,
    internal: bool,
    mac: String,
}

fn resolve_hostname() -> String {
    let hostname = gethostname::gethostname().to_string_lossy().into_owned();
    normalize_hostname(&hostname)
        .map(str::to_owned)
        .unwrap_or_else(|| format_error("hostname unavailable"))
}

fn resolve_local_ip() -> String {
    load_interfaces()
        .and_then(|interfaces| select_primary_address(&interfaces))
        .map(|candidate| candidate.ip.to_string())
        .unwrap_or_else(|| format_error("no valid local IP found"))
}

fn resolve_mac() -> String {
    load_interfaces()
        .and_then(|interfaces| {
            let preferred = select_primary_address(&interfaces);
            select_mac(&interfaces, preferred.as_ref())
        })
        .unwrap_or_else(|| format_error("no valid MAC address found"))
}

fn load_interfaces() -> Option<Vec<InterfaceSnapshot>> {
    let interfaces = NetworkInterface::show().ok()?;
    Some(interfaces.into_iter().map(snapshot_interface).collect())
}

fn snapshot_interface(interface: NetworkInterface) -> InterfaceSnapshot {
    let addrs = interface
        .addr
        .into_iter()
        .map(|addr| match addr {
            Addr::V4(addr) => IpAddr::V4(addr.ip),
            Addr::V6(addr) => IpAddr::V6(addr.ip),
        })
        .collect();

    InterfaceSnapshot {
        name: interface.name,
        index: interface.index,
        internal: interface.internal,
        mac: interface.mac_addr,
        addrs,
    }
}

fn normalize_hostname(hostname: &str) -> Option<&str> {
    let trimmed = hostname.trim();
    (!trimmed.is_empty()).then_some(trimmed)
}

fn select_primary_address(interfaces: &[InterfaceSnapshot]) -> Option<AddressCandidate> {
    let mut candidates: Vec<_> = interfaces
        .iter()
        .flat_map(|interface| {
            interface
                .addrs
                .iter()
                .copied()
                .filter(|&ip| is_valid_local_ip(ip))
                .map(|ip| AddressCandidate {
                    interface_name: interface.name.clone(),
                    interface_index: interface.index,
                    internal: interface.internal,
                    ip,
                })
        })
        .collect();

    candidates.sort_by(compare_address_candidates);
    candidates.into_iter().next()
}

fn select_mac(
    interfaces: &[InterfaceSnapshot],
    preferred: Option<&AddressCandidate>,
) -> Option<String> {
    if let Some(preferred) = preferred
        && let Some(interface) = interfaces.iter().find(|interface| {
            interface.index == preferred.interface_index
                && interface.name == preferred.interface_name
        })
        && let Some(mac) = interface.mac.as_deref().and_then(normalize_mac)
    {
        return Some(mac);
    }

    let mut candidates: Vec<_> = interfaces
        .iter()
        .filter_map(|interface| {
            normalize_mac(interface.mac.as_deref()?).map(|mac| MacCandidate {
                interface_name: interface.name.clone(),
                interface_index: interface.index,
                internal: interface.internal,
                mac,
            })
        })
        .collect();

    candidates.sort_by(compare_mac_candidates);
    candidates.into_iter().next().map(|candidate| candidate.mac)
}

fn is_valid_local_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(addr) => !addr.is_loopback() && !addr.is_unspecified(),
        IpAddr::V6(addr) => !addr.is_loopback() && !addr.is_unspecified(),
    }
}

fn compare_address_candidates(left: &AddressCandidate, right: &AddressCandidate) -> Ordering {
    ip_family_rank(left.ip)
        .cmp(&ip_family_rank(right.ip))
        .then_with(|| left.internal.cmp(&right.internal))
        .then_with(|| left.interface_index.cmp(&right.interface_index))
        .then_with(|| {
            left.interface_name
                .to_ascii_lowercase()
                .cmp(&right.interface_name.to_ascii_lowercase())
        })
        .then_with(|| compare_ip(left.ip, right.ip))
}

fn compare_mac_candidates(left: &MacCandidate, right: &MacCandidate) -> Ordering {
    left.internal
        .cmp(&right.internal)
        .then_with(|| left.interface_index.cmp(&right.interface_index))
        .then_with(|| {
            left.interface_name
                .to_ascii_lowercase()
                .cmp(&right.interface_name.to_ascii_lowercase())
        })
        .then_with(|| left.mac.cmp(&right.mac))
}

fn ip_family_rank(ip: IpAddr) -> u8 {
    match ip {
        IpAddr::V4(_) => 0,
        IpAddr::V6(_) => 1,
    }
}

fn compare_ip(left: IpAddr, right: IpAddr) -> Ordering {
    match (left, right) {
        (IpAddr::V4(left), IpAddr::V4(right)) => left.octets().cmp(&right.octets()),
        (IpAddr::V6(left), IpAddr::V6(right)) => left.octets().cmp(&right.octets()),
        (IpAddr::V4(_), IpAddr::V6(_)) => Ordering::Less,
        (IpAddr::V6(_), IpAddr::V4(_)) => Ordering::Greater,
    }
}

fn normalize_mac(raw: &str) -> Option<String> {
    let mut hex = String::with_capacity(12);

    for ch in raw.chars() {
        if ch.is_ascii_hexdigit() {
            hex.push(ch.to_ascii_lowercase());
        } else if ch == ':' || ch == '-' || ch.is_ascii_whitespace() {
            continue;
        } else {
            return None;
        }
    }

    if hex.len() != 12 || hex.chars().all(|ch| ch == '0') {
        return None;
    }

    let mut formatted = String::with_capacity(17);
    for (idx, chunk) in hex.as_bytes().chunks(2).enumerate() {
        if idx > 0 {
            formatted.push(':');
        }
        formatted.push(chunk[0] as char);
        formatted.push(chunk[1] as char);
    }

    Some(formatted)
}

fn format_error(message: &str) -> String {
    format!("[Error: {message}]")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

    #[test]
    fn trims_hostname_output() {
        assert_eq!(normalize_hostname(" laptop-01 \r\n"), Some("laptop-01"));
        assert_eq!(normalize_hostname("   "), None);
    }

    #[test]
    fn localip_ignores_loopback_and_unspecified_ipv4() {
        let interfaces = vec![InterfaceSnapshot {
            name: "ethernet0".to_string(),
            index: 7,
            internal: false,
            mac: Some("A4-5E-60-12-AB-CD".to_string()),
            addrs: vec![
                IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)),
                IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)),
                IpAddr::V4(Ipv4Addr::new(192, 168, 1, 24)),
            ],
        }];

        assert_eq!(
            select_primary_address(&interfaces).map(|candidate| candidate.ip.to_string()),
            Some("192.168.1.24".to_string())
        );
    }

    #[test]
    fn localip_prefers_ipv4_for_primary_selection() {
        let interfaces = vec![
            InterfaceSnapshot {
                name: "wifi0".to_string(),
                index: 12,
                internal: false,
                mac: Some("A4-5E-60-12-AB-CD".to_string()),
                addrs: vec![IpAddr::V6(Ipv6Addr::new(
                    0xfe80, 0, 0, 0, 0x5a6f, 0x22ff, 0xfe11, 0x3344,
                ))],
            },
            InterfaceSnapshot {
                name: "ethernet0".to_string(),
                index: 8,
                internal: false,
                mac: Some("AA-BB-CC-DD-EE-FF".to_string()),
                addrs: vec![IpAddr::V4(Ipv4Addr::new(10, 0, 0, 17))],
            },
        ];

        assert_eq!(
            select_primary_address(&interfaces).map(|candidate| candidate.ip.to_string()),
            Some("10.0.0.17".to_string())
        );
    }

    #[test]
    fn localip_ignores_loopback_ipv6() {
        let interfaces = vec![
            InterfaceSnapshot {
                name: "loopback".to_string(),
                index: 1,
                internal: true,
                mac: None,
                addrs: vec![IpAddr::V6(Ipv6Addr::LOCALHOST)],
            },
            InterfaceSnapshot {
                name: "wifi0".to_string(),
                index: 12,
                internal: false,
                mac: Some("A4-5E-60-12-AB-CD".to_string()),
                addrs: vec![IpAddr::V6(Ipv6Addr::new(
                    0xfe80, 0, 0, 0, 0x5a6f, 0x22ff, 0xfe11, 0x3344,
                ))],
            },
        ];

        assert_eq!(
            select_primary_address(&interfaces).map(|candidate| candidate.ip.to_string()),
            Some("fe80::5a6f:22ff:fe11:3344".to_string())
        );
    }

    #[test]
    fn localip_selection_is_deterministic_with_multiple_candidates() {
        let interfaces = vec![
            InterfaceSnapshot {
                name: "wifi0".to_string(),
                index: 20,
                internal: false,
                mac: Some("de:ad:be:ef:00:01".to_string()),
                addrs: vec![IpAddr::V4(Ipv4Addr::new(192, 168, 1, 50))],
            },
            InterfaceSnapshot {
                name: "ethernet0".to_string(),
                index: 10,
                internal: false,
                mac: Some("de:ad:be:ef:00:02".to_string()),
                addrs: vec![
                    IpAddr::V4(Ipv4Addr::new(10, 0, 0, 42)),
                    IpAddr::V4(Ipv4Addr::new(10, 0, 0, 43)),
                ],
            },
        ];

        assert_eq!(
            select_primary_address(&interfaces),
            Some(AddressCandidate {
                interface_name: "ethernet0".to_string(),
                interface_index: 10,
                internal: false,
                ip: IpAddr::V4(Ipv4Addr::new(10, 0, 0, 42)),
            })
        );
    }

    #[test]
    fn mac_formatting_is_lowercase_colon_separated() {
        assert_eq!(
            normalize_mac("A4-5E-60-12-AB-CD"),
            Some("a4:5e:60:12:ab:cd".to_string())
        );
        assert_eq!(
            normalize_mac("a45e6012abcd"),
            Some("a4:5e:60:12:ab:cd".to_string())
        );
    }

    #[test]
    fn mac_rejects_all_zero_addresses() {
        assert_eq!(normalize_mac("00:00:00:00:00:00"), None);
        assert_eq!(normalize_mac("000000000000"), None);
    }

    #[test]
    fn mac_prefers_the_primary_interface_when_available() {
        let interfaces = vec![
            InterfaceSnapshot {
                name: "ethernet0".to_string(),
                index: 4,
                internal: false,
                mac: Some("AA-BB-CC-DD-EE-FF".to_string()),
                addrs: vec![IpAddr::V4(Ipv4Addr::new(10, 0, 0, 8))],
            },
            InterfaceSnapshot {
                name: "wifi0".to_string(),
                index: 2,
                internal: false,
                mac: Some("11-22-33-44-55-66".to_string()),
                addrs: vec![IpAddr::V4(Ipv4Addr::new(192, 168, 1, 8))],
            },
        ];

        let preferred = select_primary_address(&interfaces);
        assert_eq!(
            select_mac(&interfaces, preferred.as_ref()),
            Some("11:22:33:44:55:66".to_string())
        );
    }

    #[test]
    fn mac_falls_back_when_primary_interface_has_no_valid_address() {
        let interfaces = vec![
            InterfaceSnapshot {
                name: "wifi0".to_string(),
                index: 2,
                internal: false,
                mac: Some("00:00:00:00:00:00".to_string()),
                addrs: vec![IpAddr::V4(Ipv4Addr::new(192, 168, 1, 8))],
            },
            InterfaceSnapshot {
                name: "ethernet0".to_string(),
                index: 4,
                internal: false,
                mac: Some("AA-BB-CC-DD-EE-FF".to_string()),
                addrs: vec![IpAddr::V4(Ipv4Addr::new(10, 0, 0, 8))],
            },
        ];

        let preferred = select_primary_address(&interfaces);
        assert_eq!(
            select_mac(&interfaces, preferred.as_ref()),
            Some("aa:bb:cc:dd:ee:ff".to_string())
        );
    }
}
