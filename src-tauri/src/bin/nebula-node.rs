//! `nebula-node` — Nebula Host without a window, for a machine that only hosts
//! (a Raspberry Pi, a spare PC, a home server). Another Nebula Host connects to
//! it over the LAN and manages its servers as if they were local.
//!
//! ```text
//! nebula-node [serve] [--port 47400] [--name "Raspberry Pi"]   run the node
//! nebula-node pair    [--port 47400]                           show a pairing code
//! nebula-node web     on|off                                   web panel for browsers on the LAN
//! ```

use std::sync::Arc;

use nebula_host_lib::error::AppResult;
use nebula_host_lib::model::Node;
use nebula_host_lib::service::{self, InstanceService};
use nebula_host_lib::{remote, store};

struct Args {
    command: String,
    /// Words after the command (`web on`).
    rest: Vec<String>,
    port: u16,
    name: Option<String>,
}

fn parse_args() -> Args {
    let mut args = std::env::args().skip(1);
    let mut parsed = Args { command: "serve".into(), rest: Vec::new(), port: remote::DEFAULT_PORT, name: None };
    let mut seen_command = false;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--port" => parsed.port = args.next().and_then(|p| p.parse().ok()).unwrap_or(parsed.port),
            "--name" => parsed.name = args.next(),
            "-h" | "--help" => {
                println!("nebula-node [serve|pair|web on|web off] [--port 47400] [--name NAME]");
                std::process::exit(0);
            }
            other if !other.starts_with('-') && !seen_command => {
                seen_command = true;
                parsed.command = other.to_string();
            }
            other if !other.starts_with('-') => parsed.rest.push(other.to_string()),
            other => {
                eprintln!("unknown option {other}");
                std::process::exit(2);
            }
        }
    }
    parsed
}

#[tokio::main]
async fn main() {
    let args = parse_args();
    let result = match args.command.as_str() {
        "serve" => serve(args).await,
        "pair" => pair(args.port).await,
        "web" => web(args.rest.first().map(String::as_str)),
        other => {
            eprintln!("unknown command {other} (expected serve or pair)");
            std::process::exit(2);
        }
    };
    if let Err(error) = result {
        eprintln!("error: {error}");
        std::process::exit(1);
    }
}

/// Turn the web panel on or off. The running node reads it on every request.
fn web(switch: Option<&str>) -> AppResult<()> {
    let mut config = store::settings::load()?;
    match switch {
        Some("on") => config.web_panel = true,
        Some("off") => config.web_panel = false,
        _ => {
            println!("web panel: {}", if config.web_panel { "on" } else { "off" });
            return Ok(());
        }
    }
    store::settings::save(&config)?;
    if config.web_panel {
        println!("web panel on. Restart the node (sudo systemctl restart nebula-node), then open http://<this machine>:47400 in a browser on the same network");
    } else {
        println!("web panel off (takes effect when the node restarts)");
    }
    Ok(())
}

async fn pair(port: u16) -> AppResult<()> {
    let url = format!("http://127.0.0.1:{port}/local/pairing-code");
    let code: serde_json::Value = reqwest::Client::new()
        .post(&url)
        .header("X-Nebula-Local", "1")
        .send()
        .await
        .map_err(|e| nebula_host_lib::error::AppError::unexpected(format!("is nebula-node running? {e}")))?
        .json()
        .await
        .map_err(nebula_host_lib::error::AppError::unexpected)?;
    let digits = code.get("code").and_then(|c| c.as_str()).unwrap_or("?");
    println!();
    println!("  Pairing code: {} {}", &digits[..digits.len().min(3)], &digits[digits.len().min(3)..]);
    println!("  Valid for 5 minutes, single use.");
    println!("  In Nebula Host: switch machine → Connect to another machine.");
    println!();
    Ok(())
}

async fn serve(args: Args) -> AppResult<()> {
    nebula_host_lib::logging::init("node");

    let mut node = match store::nodes::load()? {
        Some(node) => node,
        None => {
            let node = Node::local(args.name.clone().unwrap_or_else(store::nodes::suggested_name));
            store::nodes::save(&node)?;
            node
        }
    };
    if let Some(name) = args.name {
        if node.name != name {
            node.name = name;
            store::nodes::save(&node)?;
        }
    }

    let service: Arc<InstanceService> = InstanceService::new(node.clone()).await?;
    service::poller::spawn(service.clone());
    nebula_host_lib::restore_sleepers(service.clone());
    nebula_host_lib::apply_default_icons(service.clone());
    let _remote = remote::serve(service.clone(), args.port).await?;

    let address = nebula_host_lib::net::lan::primary_address()
        .map(|ip| format!("{ip}:{}", args.port))
        .unwrap_or_else(|| format!("<this machine>:{}", args.port));
    tracing::info!("Nebula node \"{}\" ready at {address}", node.name);
    tracing::info!("Pair a device: run `nebula-node pair` on this machine and enter the code in Nebula Host.");

    shutdown_signal().await;
    tracing::info!("shutting down: stopping servers cleanly");
    service.wake.stop_all().await;
    service.forwarder.release_all().await;
    service.processes.stop_all("stop").await;
    Ok(())
}

async fn shutdown_signal() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{signal, SignalKind};
        let mut term = signal(SignalKind::terminate()).expect("SIGTERM handler");
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {}
            _ = term.recv() => {}
        }
    }
    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
    }
}
