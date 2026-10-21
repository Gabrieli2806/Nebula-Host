# Nebula Host

Self-hosted game-server manager for Windows. Runs game servers on your own machine and
manages them from a native desktop app (Tauri v2 + React).

Server management needs no account or cloud. The optional Launcher section
supports Microsoft sign-in for Minecraft Java profiles on this device.

## Launcher (development)

The `Launcher` section is available in the desktop and Android navigation:
local profile configurations, the official Minecraft version list, a client-side
Modrinth catalog (mods, modpacks, resource packs and shaders), and session-only
Microsoft/Xbox/Minecraft Java account verification. UI copy is available in English
and Spanish. Launcher commands stay local even when managing a remote server.

**Windows x64 can install and play Vanilla releases from 1.20.5 onward.** Choose
the official demo without an account, or a verified Java account after Microsoft
application setup. The engine verifies downloads, prepares Java, isolates profile
files and supervises the game process, with progress, cancellation, repair and logs.
Mod-loader installation and modpack import remain pending. Android still needs a
native graphical runtime and touch controls. Account tokens stay in Rust memory;
persistent accounts and refresh require platform secure storage.

Microsoft login uses Nebula Host's own public-client application registration
(device authorization, personal accounts), approved by Mojang for Minecraft
services. It is fixed in `src-tauri/src/launcher/auth.rs` and cannot be changed
from the app. Forks must register and get their own application approved: never
reuse another launcher's application ID, and never add a client secret.
See [docs/LAUNCHER.md](docs/LAUNCHER.md) for architecture, references and remaining work.

For local testing, `tauri dev -- -- --launcher-preview` opens a debug-only window
with separate app data under `nebula-launcher-preview`, leaving an existing host
installation alone. `--launcher` opens the Launcher in the regular application.
`cargo run -p nebula-host --bin nebula-launcher --no-default-features -- demo 1.21.4 --smoke`
exercises the same engine against the official demo without passing account tokens
through a command line.

## Layout

| Path | What |
|---|---|
| `src/` | React 19 + TypeScript frontend (Vite 7, Tailwind 4) |
| `src-tauri/` | Rust backend: game manifests, runtimes, networking, IPC |
| `crates/nebula-downloader/` | Resumable HTTP downloads with checksum verification |
| `docs/` | Architecture notes and decisions |
| `design-proposals/` | Visual direction canvases |

## Architecture in one paragraph

The app manages a **list of nodes** (today: exactly one, this machine). A node hosts
**instances**. Every instance belongs to a **game**, and a game is *data* — a manifest
declaring how to install it, how to launch it, how to read players out of it, which
settings it exposes and how to back it up. Adding Valheim or Palworld is a new manifest,
not a refactor. See [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md).

## Develop

```bash
npm install
npm run tauri:dev      # desktop app with hot reload
npm run check          # typecheck + lint
cargo test --workspace # Rust unit tests
```

## Package (MSIX, for the Microsoft Store)

```powershell
npm run build
cargo build --release --manifest-path src-tauri/Cargo.toml
pwsh src-tauri/msix/build-msix.ps1
```
