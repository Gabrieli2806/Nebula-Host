# Audit: features duplicated between the server manager and the launcher

Scope: `src-tauri/src/{service,game,runtime}` + `src/features/server*` (server
side) against `src-tauri/src/launcher`, `ipc/launcher.rs`, `mcp/launcher_tools.rs`
and `src/features/launcher` (client side). Each finding says what is
duplicated, what differed, and what was done.

Status: **fixed** in this change · **open** (proposal kept for a follow-up).

| # | Finding | Severity | Status |
|---|---|---|---|
| 1 | Two Modrinth clients: `game/minecraft/modrinth.rs` (server) and `launcher/content.rs` + `launcher::search` (client). The launcher copy lacks caching, 429 handling, the exact-version re-check, loader ranking, hash-based "already installed" and `commit_jar`. | bug | **partly fixed**: the launcher no longer offers older builds as updates (same rule as `modrinth::is_newer`, except when the installed file does not fit the instance). Merging into one client (an `AddonProfile` with a client/server side) is **open**: it changes the JSON the launcher UI receives. |
| 2 | A third, deprecated Modrinth client (`AddonClient`) still exposed over IPC: `addon_install` downloaded **any URL from the webview, without host check or checksum**. Nothing called it. | security | **fixed**: client, its three IPC commands, remote-dispatch entries, TS wrappers and types removed. `game/minecraft/addons.rs` keeps only the enable/disable helpers and reuses `modrinth::DISABLED_SUFFIX`. |
| 3 | Game stdout reader in the launcher (`lines().map_while(Result::ok)`) stopped at the first non-UTF-8 line, leaving the pipe undrained (the game can block). The server already had `runtime::console::read_line_lossy`. | bug | **fixed**: the launcher uses `read_line_lossy`. |
| 4 | The launcher downloaded Temurin without the per-major `DownloadGuard` the server uses, so a launcher and a server download of the same Java could unpack over each other. | bug | **fixed**: `service::java::ensure_java` takes the same lock, or waits for the running download and uses its result. |
| 5 | Download errors classified seven ways. `downloader.rs` looked for "checksum"/"hash", but the crate says "integrity check failed", so server-jar checksum failures were reported as generic download failures. Launcher callers threw the detail away. | bug | **fixed**: `nebula_downloader::is_integrity_error` + `error::download_error` used by every caller. |
| 6 | The shared HTTP client has a 120 s *total* timeout, applied to downloads of up to 1 GB (modpacks, server jars, libraries). | bug | **fixed**: `game::download_client()` (connect 10 s, 60 s of silence, no total limit) for every `download_file` caller. |
| 7 | "Download, verify, rename" written four times, five SHA-1 file hashers, temp names that replace the extension (`a.jar`/`a.zip` collide). | maint | open |
| 8 | Server Forge/NeoForge installers downloaded with `checksum: None`; the launcher already verified Maven's `.sha1`. Installer URLs written twice. | bug | **fixed**: `downloader::{forge_installer_url, neoforge_installer_url, maven_sha1}` shared by both. |
| 9 | Mojang version manifest fetched in three places (v1 and v2), uncached. | maint | open |
| 10 | Atomic file writes implemented ~13 times, several without fsync, some not atomic. | maint | open (`store::paths::write_atomic`) |
| 11 | Dropping files into a server cleared caches by hand and missed the console completion cache. | bug | **fixed**: uses `after_folder_change`. |
| 12 | Instance folder path and `ContentKind`/`Loader` ↔ tag mappings repeated (6 and 4 places). | maint | open |
| 13 | MCP tool scaffolding (`Tool`, `invalid`, `str_arg`, `pretty`, resolve-by-name) copied between `mcp/tools.rs` and `mcp/launcher_tools.rs`. | maint | open |
| 14 | Two "share log on mclo.gs" flows; the launcher's lacks crash reports and trimming notice. | maint | open |
| 15 | Add-on browser and installed list written twice in the UI (`Browse.tsx`/`Content.tsx` vs `addons/BrowseTab.tsx`/`InstalledTab.tsx`). | maint | open |
| 16 | Front-end format helpers duplicated (`relativeDate`/`formatRelative`, `compactCount`/`formatCount`, `memory / 1024 GB`/`formatMegabytes`, four `FileReader` wrappers). | maint | **partly fixed**: the launcher uses `formatMegabytes`. |
| 17 | Launcher query keys inline (34 copies) and `toAppError` wrapped by hand 31 times. | maint | **partly fixed**: launcher queries go through `features/launcher/hooks.ts`. |
| 18 | Confirmation dialogs rebuilt ~10 times; `StatusChip` copied inline. | maint | open |
| 19 | CSS: launcher keyframes duplicate `transfer-slide`/`nebula-turn`; 27 unused launcher classes. | maint | **fixed** for the launcher (stylesheet rewritten, dead rules removed). |
| 20–23 | Base64 file import vs paths, icon normalisation, log tail reading, small in-file repeats. | maint | open |

## Not duplicates (keep separate)

Process supervision (server `ProcessManager` vs launcher `ClientEngine`),
instance persistence models, launcher-only token redaction, Java *selection*
(already shared through `runtime::java`), per-upstream download host
allowlists, Fabric loader listing endpoints, launcher IPC routing (must never
go to a remote node), server drop-upload pipeline (servers may be remote),
and content icons (read from a mod) vs server icons (written for the game).
