# Nebula Host

Self-hosted game-server manager for Windows. Runs game servers on your own machine and
manages them from a native desktop app (Tauri v2 + React).

No account, no login, no cloud. The machine is the product.

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
