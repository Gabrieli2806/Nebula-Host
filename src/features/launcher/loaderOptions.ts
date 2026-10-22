import type { Loader } from "./types";

export const LOADERS: { id: Loader; name: string; glyph: string }[] = [
  { id: "vanilla", name: "Vanilla", glyph: "cube" },
  { id: "fabric", name: "Fabric", glyph: "spool" },
  { id: "forge", name: "Forge", glyph: "hammer" },
  { id: "neoforge", name: "NeoForge", glyph: "flame" },
  { id: "quilt", name: "Quilt", glyph: "sheet" },
];

export function loaderInfo(id: Loader): { id: Loader; name: string; glyph: string } {
  return LOADERS.find(loader => loader.id === id) ?? { id: "vanilla", name: "Vanilla", glyph: "cube" };
}
