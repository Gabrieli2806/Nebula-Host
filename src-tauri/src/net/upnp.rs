//! Asking the router to forward a port (UPnP/IGD).
//!
//! Opt-in only: forwarding a port exposes a server to the internet, so nothing
//! here runs unless the user asked for it.

use std::collections::HashMap;
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
use std::sync::Arc;
use std::time::Duration;

use igd_next::aio::tokio as igd_tokio;
use igd_next::{PortMappingProtocol, SearchOptions};
use tokio::sync::RwLock;
use tokio::task::JoinHandle;
use tracing::{info, warn};

use crate::error::{AppError, AppResult, ErrorCode};
use crate::net::lan;

/// Lease length asked of the router.
///
/// Deliberately not permanent: some routers fail *every* `DeletePortMapping`
/// call, and a mapping we cannot delete would outlive the server forever. With
/// a lease, the worst case is that it expires on its own.
const LEASE_SECONDS: u32 = 3600;
/// Renew comfortably before expiry so a slow router cannot drop the mapping.
const RENEW_AFTER: Duration = Duration::from_secs(LEASE_SECONDS as u64 / 2);
const SEARCH_TIMEOUT: Duration = Duration::from_secs(5);

/// A live forwarding, kept alive by a renewal task.
struct Forwarding {
    port: u16,
    renewer: JoinHandle<()>,
}

#[derive(Default)]
pub struct PortForwarder {
    active: Arc<RwLock<HashMap<String, Forwarding>>>,
}

impl PortForwarder {
    pub fn new() -> Self {
        Self::default()
    }

    /// Forward `port` to this machine and keep the lease renewed.
    pub async fn forward(&self, instance_id: &str, port: u16, description: &str) -> AppResult<()> {
        let local = lan::primary_address()
            .ok_or_else(|| AppError::new(ErrorCode::RouterNotFound).detail("no LAN address"))?;

        add_mapping(local, port, description).await?;

        let renewer = {
            let local = local;
            let description = description.to_string();
            tokio::spawn(async move {
                loop {
                    tokio::time::sleep(RENEW_AFTER).await;
                    if let Err(error) = add_mapping(local, port, &description).await {
                        warn!(port, %error, "could not renew the port mapping");
                    }
                }
            })
        };

        let mut active = self.active.write().await;
        if let Some(previous) = active.insert(instance_id.to_string(), Forwarding { port, renewer })
        {
            previous.renewer.abort();
        }
        Ok(())
    }

    /// Stop renewing and ask the router to drop the mapping. A router that
    /// refuses the delete is not an error the user can act on — the lease
    /// takes care of it — so it is logged, not surfaced.
    pub async fn release(&self, instance_id: &str) {
        let Some(forwarding) = self.active.write().await.remove(instance_id) else { return };
        forwarding.renewer.abort();

        match gateway().await {
            Ok(gateway) => {
                if let Err(error) =
                    gateway.remove_port(PortMappingProtocol::TCP, forwarding.port).await
                {
                    warn!(port = forwarding.port, %error, "router refused to delete the mapping; the lease will expire it");
                }
            }
            Err(error) => {
                warn!(port = forwarding.port, %error, "no router to release the mapping on");
            }
        }
    }

    pub async fn release_all(&self) {
        let ids: Vec<String> = self.active.read().await.keys().cloned().collect();
        for id in ids {
            self.release(&id).await;
        }
    }

    pub async fn is_forwarded(&self, instance_id: &str) -> bool {
        self.active.read().await.contains_key(instance_id)
    }
}

/// Result of the UPnP self-test shown in Settings.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpnpCheck {
    /// A UPnP router answered on the LAN.
    pub router_found: bool,
    pub router_address: Option<String>,
    /// The address the router says it has on the internet side.
    pub external_ip: Option<String>,
    /// `public`, `private` (double NAT: another router in front) or `cgnat`
    /// (the provider shares one public IP between customers).
    pub external_kind: Option<String>,
    /// A test mapping was created and removed successfully.
    pub mapping_works: bool,
    pub local_ip: Option<String>,
    /// Error code of the step that failed, if any.
    pub failed_step: Option<String>,
    pub detail: Option<String>,
}

fn classify(ip: std::net::IpAddr) -> &'static str {
    match ip {
        std::net::IpAddr::V4(v4) => {
            let [a, b, ..] = v4.octets();
            if a == 100 && (64..=127).contains(&b) {
                "cgnat"
            } else if v4.is_private() || v4.is_loopback() || v4.is_link_local() || v4.is_unspecified() {
                "private"
            } else {
                "public"
            }
        }
        std::net::IpAddr::V6(_) => "public",
    }
}

/// Self-test: find the router, ask for its external address, and create then
/// delete a short-lived test mapping. Never leaves anything behind for long:
/// the test lease is 60 seconds even if the delete fails.
pub async fn check() -> UpnpCheck {
    let local = lan::primary_address();
    let mut result = UpnpCheck {
        router_found: false,
        router_address: None,
        external_ip: None,
        external_kind: None,
        mapping_works: false,
        local_ip: local.map(|ip| ip.to_string()),
        failed_step: None,
        detail: None,
    };
    let gateway = match gateway().await {
        Ok(gateway) => gateway,
        Err(error) => {
            result.failed_step = Some("router_not_found".into());
            result.detail = error.detail;
            return result;
        }
    };
    result.router_found = true;
    result.router_address = Some(gateway.addr.to_string());

    match gateway.get_external_ip().await {
        Ok(ip) => {
            result.external_ip = Some(ip.to_string());
            result.external_kind = Some(classify(ip).to_string());
        }
        Err(error) => result.detail = Some(error.to_string()),
    }

    let Some(local) = local else {
        result.failed_step = Some("no_lan_address".into());
        return result;
    };
    let test_port: u16 = 49_000 + (std::process::id() % 1000) as u16;
    match gateway
        .add_port(
            PortMappingProtocol::TCP,
            test_port,
            SocketAddr::V4(SocketAddrV4::new(local, test_port)),
            60,
            "Nebula Host UPnP test",
        )
        .await
    {
        Ok(()) => {
            result.mapping_works = true;
            let _ = gateway.remove_port(PortMappingProtocol::TCP, test_port).await;
        }
        Err(error) => {
            result.failed_step = Some("mapping_refused".into());
            result.detail = Some(error.to_string());
        }
    }
    result
}

/// The router's external address, when it will tell us.
pub async fn external_address() -> AppResult<std::net::IpAddr> {
    gateway()
        .await?
        .get_external_ip()
        .await
        .map_err(|error| AppError::new(ErrorCode::PortForwardFailed).detail(error))
}

async fn add_mapping(local: Ipv4Addr, port: u16, description: &str) -> AppResult<()> {
    let gateway = gateway().await?;
    gateway
        .add_port(
            PortMappingProtocol::TCP,
            port,
            SocketAddr::V4(SocketAddrV4::new(local, port)),
            LEASE_SECONDS,
            description,
        )
        .await
        .map_err(|error| {
            AppError::new(ErrorCode::PortForwardFailed).with("port", port).detail(error)
        })?;

    info!(port, %local, "port forwarded");
    Ok(())
}

/// Find the router.
///
/// `SearchOptions::default()` binds `0.0.0.0`, and on a machine with VMware,
/// Hyper-V or WSL adapters the discovery datagram leaves through a virtual
/// interface and the search silently times out. Binding the address that
/// actually routes to the gateway is what makes this work on real machines.
async fn gateway() -> AppResult<igd_next::aio::Gateway<igd_tokio::Tokio>> {
    let bind_addr = match lan::primary_address() {
        Some(addr) => SocketAddr::new(addr.into(), 0),
        None => SocketAddr::new(Ipv4Addr::UNSPECIFIED.into(), 0),
    };

    igd_tokio::search_gateway(SearchOptions {
        bind_addr,
        timeout: Some(SEARCH_TIMEOUT),
        ..SearchOptions::default()
    })
    .await
    .map_err(|error| AppError::new(ErrorCode::RouterNotFound).detail(error))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_lease_is_finite_and_renewed_early() {
        assert!(LEASE_SECONDS > 0, "a permanent mapping can outlive the app forever");
        assert!(
            RENEW_AFTER < Duration::from_secs(LEASE_SECONDS as u64),
            "renewal must happen before the lease expires"
        );
    }

    #[tokio::test]
    async fn releasing_an_unknown_instance_is_harmless() {
        let forwarder = PortForwarder::new();
        forwarder.release("never-forwarded").await;
        assert!(!forwarder.is_forwarded("never-forwarded").await);
    }
}
