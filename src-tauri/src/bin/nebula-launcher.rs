//! Local diagnostic entry point for the same engine used by the desktop UI.
//! Only official demo/install modes: account tokens never go through a CLI.
use nebula_host_lib::launcher::{
    client::{ClientEngine, Credentials},
    Loader, Profile,
};
use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let mode = args.first().map(String::as_str).unwrap_or("install");
    if !matches!(mode, "install" | "demo") {
        return Err("Usage: nebula-launcher [install|demo] [Minecraft release] [--smoke]".into());
    }
    let version = args
        .get(1)
        .filter(|v| !v.starts_with('-'))
        .cloned()
        .unwrap_or_else(|| "1.21.4".into());
    let input = nebula_host_lib::launcher::CreateProfile {
        name: "Desktop demo test".into(),
        game_version: version.clone(),
        loader: Loader::Vanilla,
        memory_mb: 2048,
    };
    nebula_host_lib::launcher::validate_profile(&input)?;
    let id = uuid::Uuid::new_v3(
        &uuid::Uuid::NAMESPACE_OID,
        format!("nebula-demo-{version}").as_bytes(),
    );
    let profile = Profile {
        id,
        name: input.name,
        game_version: version,
        loader: Loader::Vanilla,
        memory_mb: 2048,
    };
    let engine = Arc::new(ClientEngine::default());
    let credentials = (mode == "demo").then(|| Credentials {
        name: "DemoPlayer".into(),
        uuid: "00000000000000000000000000000000".into(),
        token: "0".into(),
        demo: true,
        expires: None,
    });
    engine.start(profile, credentials).await?;
    let mut previous = String::new();
    let mut running_at = None;
    loop {
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        let statuses = engine.statuses().await;
        let Some(status) = statuses.get(&id) else {
            continue;
        };
        let label = format!(
            "{} {} {}/{}",
            status.phase, status.stage, status.completed, status.total
        );
        if label != previous {
            println!("{label}");
            previous = label;
        }
        if status.phase == "failed" {
            for line in engine.logs(id).iter().rev().take(25).rev() {
                eprintln!("{line}");
            }
            return Err(format!("{:?}", status.error).into());
        }
        if status.phase == "ready" {
            println!("Completed");
            break;
        }
        if status.phase == "running" {
            let start = running_at.get_or_insert_with(std::time::Instant::now);
            if args.iter().any(|a| a == "--smoke") && start.elapsed().as_secs() >= 45 {
                for line in engine.logs(id).iter().rev().take(15).rev() {
                    println!("{line}");
                }
                println!("Game process remains running after 45 seconds");
                break;
            }
        }
    }
    Ok(())
}
