# Architecture

## The shape

```
model/      nodes and instances — no game anywhere in here
game/       manifests (data) + adapters (the little that cannot be data)
runtime/    starting processes, reading their console, containers, JVMs
net/        ports, LAN address, router forwarding, wake-on-join
store/      settings, instance records, path safety
service/    the rules that tie the above together
ipc/        a thin, typed door for the UI
```

Dependencies point downward only. `service` may use `game`, `runtime`, `net`
and `store`; none of them know `service` exists. `ipc` knows only `service`.

## Games are data

A game ships as a JSON manifest (`src-tauri/games/*.json`) declaring:

| Field | What it decides |
|---|---|
| `distributions` | Which flavours can be installed, and which take mods |
| `runtime` | What the host needs (a JVM of version N, a native binary, an image) |
| `ports` | Which ports it uses, and which one players connect to |
| `console` | Regexes for "ready", joins, leaves, tick rate, and noise to hide |
| `players` | Where the allow/op/ban lists live |
| `settings` | Every user-editable field: type, range, help text, group |
| `backup` | What to include, what to skip, whether it must be stopped first |
| `wake` | Whether joining can wake it, and what the sleeping listing says |

Only what a manifest genuinely cannot express lives in code, behind
`GameAdapter`: talking to version APIs, and running a vendor installer. Adding
Valheim means a manifest plus one adapter with two methods — not a refactor.

**The rule:** nothing outside `game::<name>` may branch on a game id. If you
find yourself writing `if game == "minecraft"`, the manifest is missing a
field.

## Nodes

The app manages a *list* of nodes. Today there is exactly one, this machine,
with `NodeTransport::Local`. That single decision keeps two future features
cheap:

- The mobile app pairs with a node.
- A rented box is a node whose hardware the user does not own — same UI, same
  API, not a second product.

Identity is pairing, not login: a short code, LAN-first, that **expires**. See
`node/pairing.rs`.

### Moving a server between nodes

Node ids are UUIDs (records with the old shared id `"local"` are migrated once
on load, instance records included). A transfer is run by the desktop, which
may be the source, the target or a relay; bytes go source → desktop backend →
target, never through the webview (`GET /download/{exportId}`, `POST /upload`).
The rules live in `service/transfer.rs`: the source stops and locks the
server and exports it with a SHA-256; the target verifies and adopts it at
`generation + 1`; only then does the source mark its copy `transferred_out`
(or delete it). An older copy never starts unless forced, which forks it
(`forked_from`); an active copy is never overwritten by an incoming one.

## One entry point

There is one build profile. No cloud branch, no `localOnlyDesktop`
conditionals, and no `cloud-api` imports. If a hosted build ever exists, it
swaps the node's transport at the top and nothing below changes.

## Status has exactly one owner

`service::instances::set_status` is the only function that writes an instance's
status, and `ui/StatusChip.tsx` is the only component that renders it. The
poller publishes *numbers*; it never infers status from process liveness,
because a sleeping server has no process and would flip back to offline every
tick.

## Errors

The backend never returns a sentence. It returns `AppError { code, params,
detail }`, and the UI maps the code to a title, a cause and a suggested action
in the user's language (`src/i18n/*`). `detail` is raw upstream text and lives
behind a disclosure.

## Security posture

The LAN is the trust boundary, so:

- Loopback by default. `listen_on_lan` is off until the user turns it on, with
  the consequence stated next to the switch.
- Every user-supplied path goes through `store::paths::resolve_inside`.
  Absolute paths, `..`, drive prefixes and escaping symlinks are all refused.
- Port forwarding is opt-in and leased, never permanent.
- Pairing codes expire in five minutes and work once.
- Remote access is not shipped. When it is, it needs real authentication —
  pairing alone is not enough for an internet-exposed service.

## Things that were painful once

Each of these has a regression test; do not undo them.

1. **Tailwind 4 layers.** Utilities live in `@layer utilities`; an *unlayered*
   rule beats all of them regardless of specificity. Theme rules go in
   `@layer components`. `:where()` does not help.
2. **NeoForge versions.** NeoForge `21.0.x` is Minecraft **1.21**, not 1.21.0,
   and the match prefix needs its trailing dot (`21.0.`) or MC 1.21 matches
   every `21.x.y`.
3. **Forge arg files.** `win_args.txt` must be passed relative to the server
   root, not to the nested `libraries/…` directory it was found in.
4. **Tick-rate commands differ.** Forge answers `forge tps`, NeoForge only
   `neoforge tps`, and only the `Overall` row is the server-wide figure.
5. **Wake proxy duplicate spawn.** The liveness check and the registration
   happen under one lock, or a second unbindable listener aborts the working
   one and the server can never be woken.
6. **Port release probing.** Close the socket first, probe from another task.
7. **UPnP on multi-NIC machines.** Bind discovery to the address that routes to
   the gateway; `0.0.0.0` leaves through a VMware/WSL adapter and times out
   silently. Use a lease, because some routers refuse every delete.
8. **Offline-mode UUIDs are v3.** Look skins up by name for those.
9. **Status clobbering.** See "Status has exactly one owner" above.
