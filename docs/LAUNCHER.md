# Launcher

The launcher manages Minecraft: Java Edition instances on **this device**. It is a
separate workspace from server instances: its records, accounts and game files
never mix with a server's, and its commands are never routed to another node.

## What it does today

- **Instances**: Vanilla, Fabric, Forge and NeoForge, each with its own game
  folder, memory and picture. Quilt can be saved as a configuration; its
  installer is not written yet. The version and loader can be changed in place.
- **Content**: Modrinth search for mods, resource packs and shaders filtered to
  the instance's version and loader; required dependencies; update and
  compatibility checks; enable/disable/remove; a `.jar`/`.zip` from disk.
  Modrinth modpacks (`.mrpack`) become new instances (bounded extraction,
  relative paths only, allowed download hosts).
- **Accounts**: Microsoft device-code sign-in → Xbox Live → XSTS → Minecraft.
  One refresh token per profile is remembered (`launcher/vault.rs`: DPAPI on
  Windows, the app-private folder on Android, owner-only elsewhere); access
  tokens are minted again when needed and never stored or sent over IPC.
  Skins and capes. The official demo works without an account; there is no
  offline/unlicensed mode.
- **Playing**: Windows x64 and Android arm64 (`launcher_capabilities.can_launch`).
  Downloads are SHA-1/size verified; Java is chosen by the version's major and
  installed when missing (Temurin on Windows, bundled on Android). Progress,
  cancel, repair, logs (shareable to mclo.gs, redacted), screenshots, desktop
  and home-screen shortcuts.
- **Android engine**: the game runs in its own process and task through a module
  built from the Amethyst-Android submodule (`src-tauri/android/game-runtime`),
  with its own bundled OpenJDK, LWJGL and renderers. On Android the client skips
  Mojang's LWJGL and native libraries. See `android-launcher-graphics.md` and
  `android-console-mode.md`.
- **Assistants (MCP)**: local tools to list and manage instances; launching with
  the user's account is a separate opt-in.

## Where it lives

| Part | Path |
|---|---|
| Screens | `src/features/launcher/` (routes in `panels.ts`: `/launcher`, `/launcher/:section`, `/launcher/instances/:id/:panel`) |
| Sidebar / tab bar while an instance is open | `LauncherRail.tsx`, `src/app/MobileTabs.tsx` |
| Backend | `src-tauri/src/launcher/` (`client.rs` engine, `loaders.rs`, `content.rs`, `auth.rs`, `vault.rs`, `media.rs`, `android.rs`) |
| IPC | `src-tauri/src/ipc/launcher.rs` (local only; not in `remote/dispatch.rs`) |
| Android engine | `src-tauri/android/game-runtime/`, `src-tauri/android/amethyst` (submodule) |

## Still open

1. Quilt installer; Linux and macOS game launch.
2. Real-device matrix: child accounts and Xbox policy failures, revoked
   consent, interrupted downloads, GPU/driver differences on Android, repeated
   launch/exit and background/resume cycles. Do not advertise every mod or
   shader as Android-compatible.
3. Publishing the Android launcher on Google Play: the game, loaders and mods
   are JARs downloaded at run time. See `google-play-proposal.md`.

## Reference review

- [Amethyst Android](https://github.com/AngelAuraMC/Amethyst-Android), branch
  `v3_openjdk`: Android architecture and
  `app_pojavlauncher/src/main/java/net/kdt/pojavlaunch/authenticator/microsoft/MicrosoftBackgroundLogin.java`
  for the Microsoft/Xbox/Minecraft boundary. Its repository LICENSE is LGPL-3.0;
  check individual native components before future engine integration.
- [Modrinth](https://github.com/modrinth/code), `apps/app/src/api/auth.rs`: separation
  between account commands and the Minecraft authentication service. The app's
  license is GPL-3.0; other directories have their own licenses.
- [Modrinth search API](https://docs.modrinth.com/api/operations/searchprojects/):
  project types, client-side facets, pagination and search parameters.
- [Microsoft device authorization protocol](https://learn.microsoft.com/en-us/entra/identity-platform/v2-oauth2-device-code):
  public client registration, polling and error semantics.

The launcher's own code (`src/features/launcher/`, `src-tauri/src/launcher/`) is
original and uses public protocols. The Android engine is different: the
`game-runtime` module compiles sources and native libraries from the
Amethyst-Android submodule (LGPL-3.0, pinned commit) and bundles OpenJDK builds
(GPLv2 with the Classpath Exception). Its build copies Amethyst's LICENSE into
the package's `assets/licenses/`; any redistribution must keep those notices and
offer the corresponding source.

## Validation performed when the engine first landed (October 2026)

Kept as a record; the launcher has grown since (see above).

- `npm run check`: passed; existing Fast Refresh warnings remain.
- `npm run build`: passed.
- `cargo check -p nebula-host --lib`: passed on Windows x64.
- `cargo check -p nebula-host --lib --target aarch64-linux-android`: passed
  with Android NDK 28.0.12674087 (library check, not an APK/device test).
- `cargo test --workspace --no-default-features --lib`: 242 passed in total,
  1 ignored; includes profile input validation and three authentication state tests.
- Live read-only requests to Modrinth search with the client facets and to
  Mojang's version manifest succeeded.

Additional PC validation: the same client engine downloaded Minecraft 1.21.4,
launched the official demo with Java 21 and `DemoPlayer`, loaded `Demo World`, and
remained running after 45 seconds. Its actual game window was inspected.
The eight launcher unit tests cover authentication state, input validation,
argument/rule expansion, cache corruption and log redaction.

Not verified: real-account OAuth completion (no application ID configured),
Android device behavior, packaged APK/MSI, loaders or modpack installation.
