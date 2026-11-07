# Nebula Host — Privacy Policy (draft)

> **Draft, not yet published.** Before it goes live:
> - fill in the publisher and contact lines below;
> - publish it at a public URL;
> - enter that URL in Google Play Console and Microsoft Partner Center, and link it from Settings › About.
>
> Everything below describes how the code in this repository behaves as of
> 07-11-2026. Review it whenever a feature that talks to the network changes.
> Not legal advice.

**Publisher:** _[name of the person or company publishing the app]_
**Contact:** _[e-mail address, not a chat or forum]_
**Last updated:** _[date of publication]_

## The short version

- Nebula Host has **no accounts of its own, no servers of its own and no analytics or telemetry**. We do not collect, receive or sell your data.
- What the app stores stays **on your devices**: the computer, phone or Raspberry Pi where you run it.
- The app talks to **third-party services only to do what you ask**: download game and server software, sign in to your Minecraft account, search mods, or share a log. Those services receive what any app needs to send them, such as your IP address and the request itself. Their own privacy policies apply.

## What stays on your device

- **Servers you host**: worlds, configuration, backups, logs and player lists. Server logs and player lists can contain other players' names, UUIDs, IP addresses and chat messages; this is written by the game server itself.
- **App settings, and the app's own log**, which you can open from Settings › Diagnostics.
- **Paired machines and devices**: the name and address of each machine you connected, and the access token it issued.
- **Launcher (optional)**:
  - your instances and their files;
  - for each Minecraft account you signed in with: its name, its UUID and a Microsoft refresh token, so you do not have to sign in again.
  - The refresh token is encrypted with Windows DPAPI on Windows, kept in the app's private storage on Android, and in an owner-only file elsewhere.
  - Access tokens are kept in memory only.

You can delete all of it by removing the app's data or uninstalling it. Server folders you placed elsewhere stay where you put them.

## What is sent, to whom, and when

| When you… | Service | What it receives |
|---|---|---|
| Create, update or repair a server | Mojang (`piston-meta.mojang.com`, `launchermeta.mojang.com`), PaperMC (`fill.papermc.io`), Purpur (`api.purpurmc.org`), Fabric (`meta.fabricmc.net`), Forge (`maven.minecraftforge.net`), NeoForged (`maven.neoforged.net`) | Download requests and your IP address |
| Install Java | Eclipse Adoptium (`api.adoptium.net`) | Download requests and your IP address |
| Play from the launcher | Mojang (`libraries.minecraft.net`, `resources.download.minecraft.net`) | Download requests and your IP address |
| Sign in to a Minecraft account, change a skin or cape | Microsoft (`login.microsoftonline.com`), Xbox Live (`user.auth.xboxlive.com`, `xsts.auth.xboxlive.com`), Minecraft services (`api.minecraftservices.com`) | The sign-in exchange you approve, then your Minecraft profile and the skin you upload |
| Search or install mods, plugins, resource packs or modpacks | Modrinth (`api.modrinth.com`, `cdn.modrinth.com`) | Your search, the Minecraft version and loader it is for, and your IP address |
| See player faces | `mc-heads.net`, `crafatar.com` | The player's name or UUID |
| Choose "Share log" | mclo.gs (Aternos GmbH) | The log you chose to share, after you confirm. It becomes **public** at the link it returns and expires after about 90 days. The app hides access tokens; read the log first, since it can contain player names and IP addresses |
| Turn on router port forwarding (UPnP, off by default) | Your router | A request to open the server's port |

**Between your own machines.**
- When you pair the app with another machine, the two talk directly over your local network.
- That connection uses plain HTTP with an access token. Use it only on a network you trust.
- Remote access, the browser panel and listening on the local network are all off until you turn them on.

**AI assistants (desktop, off by default).**
- When you turn on the MCP endpoint, an assistant on the same computer can read and manage your servers through it. It can also manage launcher instances, and start the game with your account only if you allow that separately.
- Nothing is sent anywhere by the endpoint itself.

## Children

Nebula Host is not directed to children under 13 and does not knowingly collect data from them.

## Changes and contact

When this policy changes, the date above changes with it. For questions or requests about your data, write to the contact address above.

---

_NOT AN OFFICIAL MINECRAFT PRODUCT. NOT APPROVED BY OR ASSOCIATED WITH MOJANG OR MICROSOFT._
