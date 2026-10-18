# Distribution glyphs: origin and licensing

Every glyph in `glyphs.tsx` is an original drawing made for Nebula Host. No
third-party logo, logo file or trace of one is included, so no third-party
licence or attribution applies to them. They are covered by Nebula Host's own
licence.

| Distribution | Glyph | Why this one | Store/legal verdict (review of 18-10-2026) |
|---|---|---|---|
| Vanilla | `cube` (plain isometric cube) | Neutral. Avoids the grass block, which Mojang treats as a brand asset. | **Own glyph required.** The Minecraft EULA and Usage Guidelines forbid using Minecraft logos or brand assets in third-party branding. |
| Paper | `sheet` (paper with folded corner) | Matches the name. Not the PaperMC paper-plane mark. | Official logomark allowed, unmodified and not recoloured (<https://docs.papermc.io/misc/assets/>). |
| Purpur | `pillar` (classical column) | Matches the name. | Official logo allowed under MIT, with the MIT notice (<https://github.com/PurpurMC/PurpurWebsite>). |
| Fabric | `spool` (spool of thread) | Matches the name. | Official logo allowed under MIT, with the MIT notice (<https://github.com/FabricMC/fabricmc.net>). |
| Forge | `hammer` | A forge tool. Deliberately **not** an anvil, which is the shape of Forge's mark. | **Own glyph required.** `MinecraftForge/Web` declares no licence, so all rights are reserved. |
| NeoForge | `flame` | A forge's fire. | Official logo allowed under CC BY 4.0, with attribution (<https://github.com/neoforged/websites>). |

## Why own glyphs for all six, including the allowed ones

- **Risk.** Paper's permission forbids recolouring. Its mark would have to
  appear in its own colours, next to glyphs tinted with Nebula's identity
  colours. The legal review names "own glyphs for all" as the zero-risk
  option that stays coherent with the "Nebula clásico" style.
- **Licence files.** None of the official files were vendored, so there are
  no third-party notices to carry.

## Swapping in an official mark later (only for the "allowed" ones)

1. Put the unmodified official file in this folder, with the source URL,
   licence and download date in a comment at the top of the file.
2. Add a row to this table with the attribution text. Add the same text to
   Settings › About › Licences (`about.licences*` keys in `src/i18n`).
3. Add a glyph id for it in `glyphs.tsx`, render the file there **without**
   `currentColor` (no tinting), and point the distribution's `glyph` at it
   in `src-tauri/games/minecraft.json`.

Screens never pick a glyph themselves: the manifest does. A mark can be
withdrawn by changing one manifest field back.

Never use Mojang/Minecraft, Forge, Modrinth, Eclipse Temurin/Adoptium or
mclo.gs logos. Use their names in text only.
