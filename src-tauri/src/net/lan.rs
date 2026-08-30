//! Which address on this machine other people can actually reach.

use std::net::{IpAddr, Ipv4Addr, SocketAddr, UdpSocket};

/// The LAN address that routes to the default gateway.
///
/// Enumerating interfaces and picking the first private one gets this wrong on
/// exactly the machines that matter: a PC with VMware, Hyper-V or WSL has
/// several private addresses, and the virtual one is often first. Opening a UDP
/// socket toward a public address makes the OS pick the interface it would
/// really send from — no packet is sent.
pub fn primary_address() -> Option<Ipv4Addr> {
    let socket = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)).ok()?;
    socket.connect(SocketAddr::from(([1, 1, 1, 1], 80))).ok()?;
    match socket.local_addr().ok()?.ip() {
        IpAddr::V4(addr) if !addr.is_loopback() && !addr.is_unspecified() => Some(addr),
        _ => None,
    }
}

/// Every IPv4 address this machine holds, for diagnostics and for letting the
/// user pick when the automatic choice is wrong.
pub fn all_addresses() -> Vec<(String, Ipv4Addr)> {
    if_addrs::get_if_addrs()
        .unwrap_or_default()
        .into_iter()
        .filter_map(|interface| match interface.addr.ip() {
            IpAddr::V4(addr) if !addr.is_loopback() => Some((interface.name, addr)),
            _ => None,
        })
        .collect()
}

/// True for addresses that only exist inside a private network.
pub fn is_private(addr: Ipv4Addr) -> bool {
    addr.is_private() || addr.is_link_local()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognises_private_ranges() {
        assert!(is_private(Ipv4Addr::new(192, 168, 1, 10)));
        assert!(is_private(Ipv4Addr::new(10, 0, 0, 4)));
        assert!(is_private(Ipv4Addr::new(172, 16, 3, 9)));
        assert!(is_private(Ipv4Addr::new(169, 254, 1, 1)));
        assert!(!is_private(Ipv4Addr::new(8, 8, 8, 8)));
    }

    #[test]
    fn the_primary_address_is_never_loopback() {
        // Machines in CI can be offline; only assert the shape when we get one.
        if let Some(addr) = primary_address() {
            assert!(!addr.is_loopback());
        }
    }

    #[test]
    fn listing_addresses_never_includes_loopback() {
        for (name, addr) in all_addresses() {
            assert!(!addr.is_loopback(), "{name} reported a loopback address");
        }
    }
}
