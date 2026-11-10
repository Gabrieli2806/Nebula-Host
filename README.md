# Nebula Host

Self-hosted game-server manager. Runs Minecraft: Java Edition servers on your own
machine and manages them from a native app (Tauri v2 + React), with an optional
Minecraft launcher for this device.

| Build | Platforms | What it does |
|---|---|---|
| Desktop app | Windows x64 (MSI, NSIS, MSIX), Linux x64 (deb, rpm, AppImage) | Host and manage servers; launcher |
| Headless node | Linux x64 / arm64 (`nebula-node`, systemd) | Host servers on a Pi or home server, managed from the app or its web panel |
| Android app | arm64 APK | Manage your machines remotely, host a small server on the phone, launcher |

Server management needs no account or cloud. Every machine is a *node*; the app
pairs with other nodes on your LAN with a short code that expires.

## Launcher

The optional **Launcher** section (on by default on phones, off on PCs: Settings ›
Launcher) manages Minecraft instances on this device:

- Instances with their own folder: Vanilla, Fabric, Forge and NeoForge (Quilt can
  be saved, not installed yet), version changes in place, memory, picture.
- Content from Modrinth (mods, resource packs, shaders, modpacks), update and
  compatibility checks, or a `.jar` / `.zip` from disk.
- Microsoft sign-in (device code), remembered across restarts (sealed with DPAPI on
  Windows, app-private on Android), skins and capes; or the official demo.
- Game launch on **Windows x64** and **Android arm64** (an engine built from
  [Amethyst-Android](https://github.com/AngelAuraMC/Amethyst-Android), the
  `src-tauri/android/engine` submodule). Linux can manage instances but not launch.

Launcher commands always stay on this device, even while managing another machine,
and are never exposed over the remote API.

Microsoft login uses Nebula Host's own public-client application registration
(device authorization, personal accounts), approved by Mojang for Minecraft
services. It is fixed in `src-tauri/src/launcher/auth.rs` and cannot be changed
from the app. Forks must register and get their own application approved: never
reuse another launcher's application ID, and never add a client secret.
See [docs/LAUNCHER.md](docs/LAUNCHER.md).

## Layout

| Path | What |
|---|---|
| `src/` | React 19 + TypeScript frontend (Vite 7, Tailwind 4). `app/` is the frame (title bar, sidebar, phone tab bar), `features/` one folder per screen, `ui/` shared components, `i18n/` English and Spanish |
| `src-tauri/` | Rust backend: game manifests, runtimes, networking, IPC, remote API, MCP |
| `src-tauri/android/` | Android-only parts: the game engine module, its submodule, the native Java starter |
| `src-tauri/gen/android/` | The Android Gradle project (Tauri-generated, then customised) |
| `crates/nebula-downloader/` | Resumable HTTP downloads with checksum verification |
| `scripts/` | Android helpers: bundle the Java runtimes, build the APK |
| `packaging/linux/` | Installer and systemd unit for the headless node |
| `docs/` | Architecture, launcher, Android notes and store reviews |
| `StoreGraphics/` | Store listing art; `source/` regenerates it from demo data |
| `design-proposals/` | Visual direction canvases |

## Architecture in one paragraph

The app manages a **list of nodes** (this machine plus any it is paired with). A
node hosts **instances**. Every instance belongs to a **game**, and a game is
*data* — a manifest declaring how to install it, how to launch it, how to read
players out of it, which settings it exposes and how to back it up. Adding
Valheim or Palworld is a new manifest, not a refactor. See
[docs/ARCHITECTURE.md](docs/ARCHITECTURE.md).

## Develop

```bash
npm install
npm run tauri:dev      # desktop app with hot reload
npm run check          # typecheck + lint
cargo test --workspace # Rust unit tests (desktop stack: needs webkit2gtk on Linux)
cargo test --workspace --no-default-features   # same tests, headless (no webview)
```

For local testing, `tauri dev -- -- --launcher-preview` opens a debug-only window
with separate app data under `nebula-launcher-preview`, leaving an existing host
installation alone. `--launcher` opens the Launcher in the regular application.
`cargo run -p nebula-host --bin nebula-launcher --no-default-features -- demo 1.21.4 --smoke`
exercises the client engine against the official demo.

### Android

Needs JDK 21, the Android SDK and the NDK versions pinned in
`.github/workflows/android.yml`. The game engine is a submodule and the Java
runtimes are bundled into the package (Android may not run a downloaded runtime):

```bash
git submodule update --init src-tauri/android/engine
git -C src-tauri/android/engine -c submodule.androidnsbypass.url=https://github.com/alexytomi/androidnsbypass.git submodule update --init androidnsbypass
node scripts/android-game-jre.mjs
node scripts/android-jre.mjs arm64-v8a 21,25
npx tauri android build --debug --target aarch64 --apk
```

On Windows, `scripts/build-android.ps1 -JavaHome <jdk21>` runs the same steps.
Publishing on Google Play has extra constraints; see
[docs/google-play-proposal.md](docs/google-play-proposal.md).

## Package (MSIX, for the Microsoft Store)

```powershell
npm run build
cargo build --release --manifest-path src-tauri/Cargo.toml
pwsh src-tauri/msix/build-msix.ps1
```

Releases: raising `version` in `src-tauri/tauri.conf.json` on `main` tags the
commit and builds every artifact above (`.github/workflows/release.yml`).

## Docs

- [PENDIENTE.md](PENDIENTE.md): what the last audit fixed and what is still to do (Spanish)
- [ARCHITECTURE.md](docs/ARCHITECTURE.md): layers, games as data, nodes, transfers, security
- [LAUNCHER.md](docs/LAUNCHER.md): the launcher's design, status and references
- [android-launcher-graphics.md](docs/android-launcher-graphics.md), [android-console-mode.md](docs/android-console-mode.md): Android game engine notes
- [store-compliance-review.md](docs/store-compliance-review.md): Microsoft Store, Google Play and Mojang review
- [google-play-proposal.md](docs/google-play-proposal.md): how to publish the Android app on Google Play
