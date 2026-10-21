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

**This is a foundation, not a playable launcher yet.** It does not install or launch
the Minecraft client, install loaders or import modpacks. Android still needs a
native graphical runtime and touch controls. Account tokens are kept only in Rust
memory; persistent accounts and refresh require platform secure storage.

Microsoft login requires your own public-client application registration with
device authorization enabled and permission to use Minecraft services. Supply its
application ID in the Accounts tab, or set `NEBULA_MICROSOFT_CLIENT_ID` before
starting Nebula. Never use another launcher's application ID or a client secret.
See [docs/LAUNCHER.md](docs/LAUNCHER.md) for architecture, references and remaining work.

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
