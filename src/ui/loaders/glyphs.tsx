/**
 * The distribution glyphs: Nebula Host's own drawings, one per server
 * flavour, on the same 16px grid and stroke as the icon set (Icon.tsx).
 *
 * None of these is, copies or traces a third-party logo. They are generic
 * objects chosen for the flavour's name (a cube, a sheet of paper, a column,
 * a spool of thread, a hammer, a flame) and they deliberately avoid the
 * shapes the projects use as marks (no grass block, no anvil). See
 * ATTRIBUTION.md in this folder for why, and for how an official mark could
 * be swapped in later without touching any screen.
 *
 * Each glyph is an outline in `currentColor` plus, where it helps the shape
 * read at 12px, one face filled at low opacity. Colour comes from the
 * element they sit in (a `.tag[data-identity]` gives the distribution's
 * identity ink), so they follow the theme and never carry a brand colour of
 * their own.
 */

import type { ReactNode } from "react";

export type LoaderGlyph = "cube" | "sheet" | "pillar" | "spool" | "hammer" | "flame";

const FACE = { fill: "currentColor", fillOpacity: 0.28, stroke: "none" } as const;

export const GLYPHS: Record<LoaderGlyph, ReactNode> = {
  // Vanilla: a plain isometric cube. Not the grass block (a Mojang asset).
  cube: (
    <>
      <path d="M8 1.9 13.3 5 8 8.1 2.7 5z" {...FACE} />
      <path d="M8 1.9 13.3 5v6L8 14.1 2.7 11V5zM2.7 5 8 8.1 13.3 5M8 8.1v6" />
    </>
  ),
  // Paper: a sheet with a folded corner.
  sheet: (
    <>
      <path d="M9.6 2v3.2h3.2z" {...FACE} />
      <path d="M3.6 2h6l3.2 3.2V14H3.6zM9.6 2v3.2h3.2M5.9 8.4h4.6M5.9 11h3.2" />
    </>
  ),
  // Purpur: a classical column.
  pillar: (
    <>
      <path d="M2.8 2.4h10.4v2.2H2.8z" {...FACE} />
      <path d="M2.8 2.4h10.4v2.2H2.8zM4.4 4.6v7M11.6 4.6v7M6.8 4.6v7M9.2 4.6v7M2.8 11.6h10.4v2H2.8z" />
    </>
  ),
  // Fabric: a bobbin seen from the side: two round flanges, a waisted core
  // and a loose end of thread. Drawn with curves and no vertical strokes, so
  // at 12px it never reads as Purpur's fluted column.
  spool: (
    <>
      <path d="M5.2 4.1Q7.4 8 5.2 11.9h5.6Q8.6 8 10.8 4.1z" {...FACE} />
      <path d="M3 3a5 1.3 0 1 0 10 0a5 1.3 0 1 0-10 0M3 13a5 1.3 0 1 0 10 0a5 1.3 0 1 0-10 0M5.2 4.1Q7.4 8 5.2 11.9M10.8 4.1Q8.6 8 10.8 11.9M9.4 8c2 0 3.8.9 4.6 2.6" />
    </>
  ),
  // Forge: a hammer. Not an anvil, which is the shape of Forge's own mark.
  hammer: (
    <g transform="rotate(-40 8 8)">
      <path d="M3.4 2.8h9.2v3.6H3.4z" {...FACE} />
      <path d="M3.4 2.8h9.2v3.6H3.4zM7 6.4h2V14H7z" />
    </g>
  ),
  // NeoForge: a flame, as from a forge's hearth.
  flame: (
    <>
      <path d="M8 14.2c-1.3 0-2.2-.9-2.2-2.1 0-1.3 1-2 1.7-3.1 1 .9 2.7 1.7 2.7 3.2 0 1.1-.9 2-2.2 2z" {...FACE} />
      <path d="M8 14.2c-2.8 0-4.8-2-4.8-4.7 0-2.5 1.7-3.9 2.8-5.4.3 1.4 1 2.3 1.9 2.7.2-2.3.9-4 2.3-5 .2 2.5 2.6 3.9 2.6 7.7 0 2.8-2 4.7-4.8 4.7z" />
    </>
  ),
};

export function isLoaderGlyph(value: string | null | undefined): value is LoaderGlyph {
  return typeof value === "string" && Object.prototype.hasOwnProperty.call(GLYPHS, value);
}
