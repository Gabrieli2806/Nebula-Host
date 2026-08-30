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
