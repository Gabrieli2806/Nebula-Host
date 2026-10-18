//! Wake-on-join.
//!
//! When a server that has wake-on-join enabled stops cleanly, this takes over
//! its port and pretends to be a sleeping server: the server browser shows a
//! "sleeping" entry instead of an offline one, and an actual join attempt
//! starts the real server and tells the player to reconnect in a moment.
//!
//! Which handshake to speak comes from the game's manifest ([`WakeProtocol`]),
//! so a game whose clients simply open a TCP connection needs no code here.

use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::RwLock;
use tokio::task::JoinHandle;
use tracing::{error, info, warn};

use crate::game::manifest::{WakeProtocol, WakeSpec};

/// Called when a join attempt should start the server. Takes the instance id.
pub type WakeCallback = Arc<dyn Fn(String) + Send + Sync>;
/// Called to surface proxy activity in the instance's console:
/// (instance_id, level, message).
pub type LogCallback = Arc<dyn Fn(String, &'static str, String) + Send + Sync>;

#[derive(Clone)]
pub struct WakeHooks {
    pub on_wake: WakeCallback,
    pub on_log: LogCallback,
}

/// One sleeping server's listener.
struct Sleeper {
    handle: JoinHandle<()>,
}

/// Owns every wake listener, one per sleeping instance.
#[derive(Default)]
pub struct WakeProxyManager {
    proxies: Arc<RwLock<HashMap<String, Sleeper>>>,
}

impl WakeProxyManager {
    pub fn new() -> Self {
        Self::default()
    }

    /// Start listening for `instance_id`. A no-op when a live proxy is already
    /// registered for it.
    ///
    /// The liveness check and the registration happen under **one** lock. Two
    /// concurrent callers used to be able to register a second, unbindable
    /// listener whose failure then aborted the first, working one — leaving
    /// the server permanently unwakeable, with nothing in the log to say so.
    pub async fn spawn(
        &self,
        hooks: WakeHooks,
        instance_id: String,
        bind: IpAddr,
        port: u16,
        spec: WakeSpec,
        display_name: String,
    ) {
        let mut proxies = self.proxies.write().await;

        if proxies.get(&instance_id).is_some_and(|sleeper| !sleeper.handle.is_finished()) {
            return;
        }

        let handle = tokio::spawn(run(
            hooks,
            instance_id.clone(),
            SocketAddr::new(bind, port),
            spec,
            display_name,
        ));

        if let Some(previous) = proxies.insert(instance_id, Sleeper { handle }) {
            previous.handle.abort();
        }
    }

    /// Stop the proxy for an instance. True when one was running.
    pub async fn stop(&self, instance_id: &str) -> bool {
        let mut proxies = self.proxies.write().await;
        match proxies.remove(instance_id) {
            Some(sleeper) => {
                sleeper.handle.abort();
                info!(instance_id, "wake-on-join proxy stopped");
                true
            }
            None => false,
        }
    }

    pub async fn is_sleeping(&self, instance_id: &str) -> bool {
        self.proxies
            .read()
            .await
            .get(instance_id)
            .is_some_and(|sleeper| !sleeper.handle.is_finished())
    }

    pub async fn stop_all(&self) {
        let mut proxies = self.proxies.write().await;
        for (_, sleeper) in proxies.drain() {
            sleeper.handle.abort();
        }
    }
}

/// Bind the listener, retrying briefly.
///
/// A first failure is expected rather than exceptional: the OS often has not
/// released the port yet when the server process has only just exited. Giving
/// up immediately would leave the server unwakeable, which is worse than
/// waiting a few seconds.
async fn bind_with_retry(addr: SocketAddr, instance_id: &str) -> Option<TcpListener> {
    const ATTEMPTS: u32 = 12;

    for attempt in 1..=ATTEMPTS {
        match TcpListener::bind(addr).await {
            Ok(listener) => return Some(listener),
            Err(error) if attempt < ATTEMPTS => {
                warn!(instance_id, %addr, %error, attempt, "wake proxy bind failed; retrying");
                tokio::time::sleep(Duration::from_millis(500)).await;
            }
            Err(error) => {
                error!(instance_id, %addr, %error, "wake proxy could not bind; joining will not wake this server");
            }
        }
    }

    None
}

async fn run(
    hooks: WakeHooks,
    instance_id: String,
    addr: SocketAddr,
    spec: WakeSpec,
    display_name: String,
) {
    let Some(listener) = bind_with_retry(addr, &instance_id).await else {
        (hooks.on_log)(
            instance_id,
            "WARN",
            format!(
                "Wake-on-join could not listen on port {} — something else is using it. Start the server manually to recover.",
                addr.port()
            ),
        );
        return;
    };
    info!(instance_id, %addr, "wake-on-join proxy listening");

    let motd = spec.sleeping_motd.replace("{name}", &display_name);
    let mut consecutive_errors: u32 = 0;

    loop {
        let (mut stream, peer) = match listener.accept().await {
            Ok(conn) => {
                consecutive_errors = 0;
                conn
            }
            Err(error) => {
                consecutive_errors += 1;
                // Back off rather than spin: a persistent accept error would
                // otherwise burn a core and flood the log.
                if consecutive_errors == 1 || consecutive_errors % 50 == 0 {
                    warn!(instance_id, %error, consecutive_errors, "wake proxy accept error");
                }
                let backoff = 100u64 * u64::from(consecutive_errors.min(20));
                tokio::time::sleep(Duration::from_millis(backoff)).await;
                continue;
            }
        };

        let hooks = hooks.clone();
        let id = instance_id.clone();
        let motd = motd.clone();
        let protocol = spec.protocol;

        tokio::spawn(async move {
            // Cap the exchange: a half-open client must not pin the port.
            let result = tokio::time::timeout(
                Duration::from_secs(15),
                handle_connection(&mut stream, protocol, &motd, hooks, id.clone(), peer),
            )
            .await;

            match result {
                Ok(Err(error)) => warn!(instance_id = %id, %peer, %error, "wake proxy connection error"),
                Err(_) => warn!(instance_id = %id, %peer, "wake proxy connection timed out"),
                Ok(Ok(())) => {}
            }
        });
    }
}

type ConnResult = Result<(), Box<dyn std::error::Error + Send + Sync>>;

async fn handle_connection(
    stream: &mut TcpStream,
    protocol: WakeProtocol,
    motd: &str,
    hooks: WakeHooks,
    instance_id: String,
    peer: SocketAddr,
) -> ConnResult {
    match protocol {
        // Any connection at all is a wake signal.
        WakeProtocol::TcpConnect => {
            let _ = stream.shutdown().await;
            wake(hooks, instance_id, peer, String::new());
            Ok(())
        }
        WakeProtocol::MinecraftJava => minecraft_handshake(stream, motd, hooks, instance_id, peer).await,
    }
}

/// Minecraft Java Edition handshake: a status request gets the sleeping MOTD,
/// a login attempt starts the server and disconnects with an explanation.
async fn minecraft_handshake(
    stream: &mut TcpStream,
    motd: &str,
    hooks: WakeHooks,
    instance_id: String,
    peer: SocketAddr,
) -> ConnResult {
    let _packet_length = read_varint(stream).await?;
    let packet_id = read_varint(stream).await?;
    if packet_id != 0x00 {
        return Ok(());
    }

    let protocol_version = read_varint(stream).await?;
    let _address = read_mc_string(stream).await?;
    let mut port_buf = [0u8; 2];
    stream.read_exact(&mut port_buf).await?;
    let next_state = read_varint(stream).await?;

    match next_state {
        1 => respond_to_ping(stream, protocol_version, motd).await,
        2 => wake_from_login(stream, hooks, instance_id, peer).await,
        _ => Ok(()),
    }
}

async fn respond_to_ping(stream: &mut TcpStream, protocol_version: i32, motd: &str) -> ConnResult {
    // Consume the empty status-request packet first. Answering without
    // reading it leaves those bytes unread in the socket, and closing on
    // unread data sends an RST — the client then reports a reset connection
    // instead of showing the MOTD we just wrote.
    let _len = read_varint(stream).await?;
    let _packet_id = read_varint(stream).await?;

    let response = serde_json::json!({
        "version": { "name": "\u{00A7}7Sleeping", "protocol": protocol_version },
        "players": { "max": 0, "online": 0 },
        "description": { "text": motd },
        "preventsChatReports": true
    })
    .to_string();

    let mut payload = Vec::new();
    write_varint(&mut payload, 0x00);
    write_mc_string(&mut payload, &response);
    let mut packet = Vec::new();
    write_varint(&mut packet, payload.len() as i32);
    packet.extend_from_slice(&payload);
    stream.write_all(&packet).await?;

    // The client follows up with a ping it expects echoed back; without the
    // pong it shows "no connection" instead of the MOTD we just sent.
    if let Ok(9) = read_varint(stream).await {
        let mut ping = [0u8; 9];
        stream.read_exact(&mut ping).await?;
        let mut pong = Vec::new();
        write_varint(&mut pong, 9);
        pong.extend_from_slice(&ping);
        stream.write_all(&pong).await?;
    }

    Ok(())
}

async fn wake_from_login(
    stream: &mut TcpStream,
    hooks: WakeHooks,
    instance_id: String,
    peer: SocketAddr,
) -> ConnResult {
    let _len = read_varint(stream).await?;
    let _packet_id = read_varint(stream).await?;
    let player_name = read_mc_string(stream).await.unwrap_or_default();

    let message = "\u{00A7}aWaking the server up\n\u{00A7}7Reconnect in about 30 seconds…";
    let disconnect = serde_json::json!({ "text": message }).to_string();

    let mut payload = Vec::new();
    write_varint(&mut payload, 0x00);
    write_mc_string(&mut payload, &disconnect);
    let mut packet = Vec::new();
    write_varint(&mut packet, payload.len() as i32);
    packet.extend_from_slice(&payload);
    stream.write_all(&packet).await?;
    stream.flush().await?;
    // Close our side now, so this socket is not still holding the port when
    // the caller waits for it to be free.
    let _ = stream.shutdown().await;

    wake(hooks, instance_id, peer, player_name);
    Ok(())
}

fn wake(hooks: WakeHooks, instance_id: String, peer: SocketAddr, player_name: String) {
    info!(instance_id, %peer, %player_name, "wake-on-join triggered");
    let who = if player_name.is_empty() { peer.ip().to_string() } else { player_name };
    (hooks.on_log)(
        instance_id.clone(),
        "INFO",
        format!("Wake-on-join triggered by {who} — starting the server…"),
    );
    (hooks.on_wake)(instance_id);
}

/// Wait until the port can be bound again after the proxy lets it go.
///
/// Aborting the listener drops the listening socket, but connections it
/// already accepted linger briefly. Probing from a separate task — never while
/// still holding an accepted socket on that port — is what makes this
/// terminate.
pub async fn wait_for_port_release(instance_id: &str, port: u16) {
    let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port);

    for attempt in 1u8..=20 {
        match std::net::TcpListener::bind(addr) {
            Ok(listener) => {
                drop(listener);
                return;
            }
            Err(error) if attempt == 20 => {
                warn!(instance_id, port, %error, "port still busy after the wake proxy stopped; starting anyway");
            }
            Err(_) => tokio::time::sleep(Duration::from_millis(500)).await,
        }
    }
}

// ── Minecraft wire format ───────────────────────────────────────────────────

async fn read_varint(stream: &mut TcpStream) -> Result<i32, Box<dyn std::error::Error + Send + Sync>> {
    let mut result: i32 = 0;
    let mut shift: u32 = 0;
    loop {
        let mut buf = [0u8; 1];
        stream.read_exact(&mut buf).await?;
        let byte = buf[0];
        result |= ((byte & 0x7F) as i32) << shift;
        if byte & 0x80 == 0 {
            break;
        }
        shift += 7;
        if shift >= 32 {
            return Err("varint too large".into());
        }
    }
    Ok(result)
}

async fn read_mc_string(
    stream: &mut TcpStream,
) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    let len = read_varint(stream).await?;
    if !(0..=32767).contains(&len) {
        return Err("string length out of range".into());
    }
    let mut buf = vec![0u8; len as usize];
    stream.read_exact(&mut buf).await?;
    Ok(String::from_utf8_lossy(&buf).to_string())
}

fn write_varint(buf: &mut Vec<u8>, mut value: i32) {
    loop {
        let mut byte = (value & 0x7F) as u8;
        value = ((value as u32) >> 7) as i32;
        if value != 0 {
            byte |= 0x80;
        }
        buf.push(byte);
        if value == 0 {
            break;
        }
    }
}

fn write_mc_string(buf: &mut Vec<u8>, value: &str) {
    let bytes = value.as_bytes();
    write_varint(buf, bytes.len() as i32);
    buf.extend_from_slice(bytes);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    fn free_port() -> u16 {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind ephemeral port");
        listener.local_addr().expect("read ephemeral port").port()
    }

    fn noop_hooks() -> WakeHooks {
        WakeHooks { on_wake: Arc::new(|_| {}), on_log: Arc::new(|_, _, _| {}) }
    }

    fn minecraft_spec() -> WakeSpec {
        WakeSpec {
            bind_setting: None,
            protocol: WakeProtocol::MinecraftJava,
            sleeping_motd: "{name} is asleep".to_string(),
        }
    }

    async fn spawn_local(manager: &WakeProxyManager, id: &str, port: u16, hooks: WakeHooks) {
        manager
            .spawn(
                hooks,
                id.to_string(),
                IpAddr::V4(Ipv4Addr::LOCALHOST),
                port,
                minecraft_spec(),
                "Survival".to_string(),
            )
            .await;
    }

    /// The regression that left servers unwakeable: a duplicate spawn must not
    /// tear down the listener that is already working.
    #[tokio::test]
    async fn duplicate_spawn_keeps_the_working_listener() {
        let manager = WakeProxyManager::new();
        let port = free_port();

        spawn_local(&manager, "dup", port, noop_hooks()).await;
        tokio::time::sleep(Duration::from_millis(400)).await;
        spawn_local(&manager, "dup", port, noop_hooks()).await;
        tokio::time::sleep(Duration::from_millis(400)).await;

        assert!(
            std::net::TcpListener::bind(("127.0.0.1", port)).is_err(),
            "the wake proxy stopped listening after a duplicate spawn"
        );
        assert!(manager.stop("dup").await);
    }

    #[tokio::test]
    async fn stopping_the_proxy_releases_the_port() {
        let manager = WakeProxyManager::new();
        let port = free_port();

        spawn_local(&manager, "release", port, noop_hooks()).await;
        tokio::time::sleep(Duration::from_millis(400)).await;
        assert!(std::net::TcpListener::bind(("127.0.0.1", port)).is_err());

        assert!(manager.stop("release").await);
        wait_for_port_release("release", port).await;

        std::net::TcpListener::bind(("127.0.0.1", port))
            .expect("the port must be bindable once the proxy stops");
    }

    #[tokio::test]
    async fn stop_reports_false_when_nothing_is_registered() {
        let manager = WakeProxyManager::new();
        assert!(!manager.stop("never-started").await);
    }

    #[tokio::test]
    async fn a_login_attempt_wakes_the_server_exactly_once() {
        let wakes = Arc::new(AtomicU32::new(0));
        let counter = wakes.clone();
        let hooks = WakeHooks {
            on_wake: Arc::new(move |_| {
                counter.fetch_add(1, Ordering::SeqCst);
            }),
            on_log: Arc::new(|_, _, _| {}),
        };

        let manager = WakeProxyManager::new();
        let port = free_port();
        spawn_local(&manager, "login", port, hooks).await;
        tokio::time::sleep(Duration::from_millis(300)).await;

        let mut stream =
            TcpStream::connect(("127.0.0.1", port)).await.expect("connect to the wake proxy");

        // Handshake with next_state = 2 (login), then Login Start.
        let mut handshake = Vec::new();
        write_varint(&mut handshake, 0x00);
        write_varint(&mut handshake, 763);
        write_mc_string(&mut handshake, "localhost");
        handshake.extend_from_slice(&port.to_be_bytes());
        write_varint(&mut handshake, 2);
        let mut framed = Vec::new();
        write_varint(&mut framed, handshake.len() as i32);
        framed.extend_from_slice(&handshake);
        stream.write_all(&framed).await.expect("send handshake");

        let mut login = Vec::new();
        write_varint(&mut login, 0x00);
        write_mc_string(&mut login, "Steve");
        let mut login_framed = Vec::new();
        write_varint(&mut login_framed, login.len() as i32);
        login_framed.extend_from_slice(&login);
        stream.write_all(&login_framed).await.expect("send login");

        for _ in 0..20 {
            if wakes.load(Ordering::SeqCst) > 0 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }

        assert_eq!(wakes.load(Ordering::SeqCst), 1, "on_wake must fire exactly once");
        manager.stop("login").await;
    }

    #[tokio::test]
    async fn a_server_list_ping_gets_the_sleeping_motd() {
        let manager = WakeProxyManager::new();
        let port = free_port();
        spawn_local(&manager, "ping", port, noop_hooks()).await;
        tokio::time::sleep(Duration::from_millis(300)).await;

        let mut stream = TcpStream::connect(("127.0.0.1", port)).await.expect("connect");

        let mut handshake = Vec::new();
        write_varint(&mut handshake, 0x00);
        write_varint(&mut handshake, 763);
        write_mc_string(&mut handshake, "localhost");
        handshake.extend_from_slice(&port.to_be_bytes());
        write_varint(&mut handshake, 1); // status
        let mut framed = Vec::new();
        write_varint(&mut framed, handshake.len() as i32);
        framed.extend_from_slice(&handshake);
        stream.write_all(&framed).await.expect("send handshake");

        // Empty status request packet.
        stream.write_all(&[0x01, 0x00]).await.expect("send status request");

        let mut buf = vec![0u8; 512];
        let read = stream.read(&mut buf).await.expect("read status response");
        let text = String::from_utf8_lossy(&buf[..read]);
        assert!(text.contains("Survival is asleep"), "MOTD was: {text}");

        manager.stop("ping").await;
    }

    #[test]
    fn varints_round_trip() {
        for value in [0, 1, 127, 128, 255, 25565, i32::MAX] {
            let mut buf = Vec::new();
            write_varint(&mut buf, value);
            assert!(!buf.is_empty(), "{value}");
        }
    }
}
