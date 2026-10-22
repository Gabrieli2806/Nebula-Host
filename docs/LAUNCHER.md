# Launcher implementation status

Branch: `feat/launcher-cross-platform`, based on the existing `android` branch.

## Implemented

- Desktop sidebar and narrow-screen/Android bottom navigation entry.
- Library: create and remove local profile configurations, select official Mojang
  releases or snapshots, declare a loader and memory budget. A loader selection is
  configuration only; it does not imply installation or compatibility.
- Backend profile validation, UUID identifiers and serialized atomic index writes
  in the app's private configuration directory. Server instance records are untouched.
- Discovery: paginated Modrinth client-content searches for mods, modpacks,
  resource packs and shaders; project links open in the external browser.
- Accounts: device authorization using a user-provided/locally configured Microsoft
  public client ID, then Xbox Live, XSTS, Minecraft login and Java profile lookup.
  Multiple verified accounts can coexist during the current app session.
- OAuth polling respects the server interval, `slow_down`, expiration and cancellation.
  Device codes and tokens stay in backend memory. Only the user-visible code and
  verified profile identity cross IPC. Error bodies are not logged or exposed.
- Launcher IPC bypasses remote-node routing. No launcher operations are registered
  in the HTTP or MCP dispatchers. The browser panel does not expose this feature.
- English and Spanish copy, loading/error/empty states, retry and delete confirmation.
- Windows x64 Vanilla 1.20.5+: official metadata and SHA-1/size-verified client,
  libraries and asset objects; bounded native extraction; Java major selection
  with a verified Temurin download when necessary. Game arguments are expanded
  as individual arguments with OS/architecture/features rules.
- Explicit official demo mode (`--demo`) and separate verified-account mode.
  There is no unlicensed full-game/offline-account mode.
- Per-profile game directories, download progress/cancellation, install/repair,
  process supervision, intentional-stop handling and bounded redacted console output.
- Debug-only `--launcher-preview` uses separate app data and skips the production
  single-instance handoff, so it cannot adopt an existing host's servers/settings.

## Still required before calling this a stable playable launcher

1. Broaden client validation beyond Windows x64 Vanilla, add offline cache-only
   preparation, more detailed Java download progress and graceful game-stop UX.
   Stopping currently terminates the process; save and quit within Minecraft first
   when keeping world changes matters. Historical releases and snapshots are not
   offered as playable clients.
2. Loader-specific installers and pinned loader versions; compatible client
   dependency resolution, update checks and file ownership tracking.
3. `.mrpack` support: bounded extraction, traversal/symlink protection, hashes,
   environment filtering, overrides, dependency resolution and rollback. Existing
   server-only addon compatibility rules must not be used as client rules.
4. Persistent accounts: Windows credential storage and Android Keystore-backed
   encryption, token renewal, default-account selection and expired/revoked-session
   UX. This implementation intentionally requests no offline refresh scope and
   stores no refresh token. Relaunching Nebula requires signing in again.
5. Android engine: separate native game activity/process, supported ARM runtimes,
   LWJGL/JNI bridge, renderer selection, surface lifecycle, keyboard/mouse/gamepad
   and configurable touch controls. The bundled server JVM is insufficient.
6. Real-device matrix: Windows and Android builds, sign-in with approved app ID,
   owned Java account, denied/expired consent, child-account/Xbox policy failures,
   interrupted downloads, corrupt assets, GPU/runtime incompatibilities, resume
   after backgrounding and repeated launch/exit cycles. Do not advertise all mods
   or shaders as Android-compatible.

`launcher_capabilities.can_launch` is true only on Windows x64. Android continues
to expose profiles, catalog and accounts, with its native-runtime limitation stated
in the UI. The library displays instance cards with search, loader filters and
a selected-instance detail panel. Creating an instance opens a dialog to choose
the Minecraft version and loader, name it and optionally adjust memory. Non-Vanilla
loaders can be saved as configuration-only instances; their game controls remain
unavailable until their engines are implemented.

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

The implementation added in this branch is original code using public protocols;
no source files or binaries from those launchers have been vendored. Any future
reuse must include the applicable source, notices and license obligations.

## Validation performed

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
