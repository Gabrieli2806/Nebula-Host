/** Hooks and helpers shared by the addon tabs and the project dialog. */

import { useCallback, useState } from "react";

import { useI18n } from "@/i18n";
import type { AddonContext, AddonVersion, PendingAddonChange } from "@/types/api";

export type AddonView = "cards" | "list";

const VIEW_KEY = "nebula.addonsView";

function readView(): AddonView {
  try {
    return window.localStorage.getItem(VIEW_KEY) === "list" ? "list" : "cards";
  } catch {
    return "cards";
  }
}

/** Cards or list, remembered on this machine. */
export function useAddonView(): [AddonView, (view: AddonView) => void] {
  const [view, setView] = useState<AddonView>(readView);
  const change = useCallback((next: AddonView) => {
    setView(next);
    try {
      window.localStorage.setItem(VIEW_KEY, next);
    } catch {
      // Storage can be blocked; the choice still holds for this visit.
    }
  }, []);
  return [view, change];
}

export const PAGE_SIZES = [20, 40, 60, 100] as const;
const PAGE_KEY = "nebula.addonsPageSize";

/** How many results a page shows, remembered on this machine. Modrinth caps it at 100. */
export function usePageSize(): [number, (size: number) => void] {
  const [size, setSize] = useState<number>(() => {
    try {
      const stored = Number(window.localStorage.getItem(PAGE_KEY));
      return (PAGE_SIZES as readonly number[]).includes(stored) ? stored : 20;
    } catch {
      return 20;
    }
  });
  const change = useCallback((next: number) => {
    setSize(next);
    try {
      window.localStorage.setItem(PAGE_KEY, String(next));
    } catch {
      // Storage can be blocked; the choice still holds for this visit.
    }
  }, []);
  return [size, change];
}

/** `t` plus counted copy: `key_one` when the count is 1 and that key exists. */
export function useAddonCopy() {
  const { t, language } = useI18n();
  const tc = useCallback(
    (key: string, count: number, params?: Record<string, string | number>) => {
      const one = `${key}_one`;
      return t(count === 1 && t(one) !== one ? one : key, { count, ...params });
    },
    [t],
  );
  return { t, tc, language };
}

const TYPE_RANK: Record<string, number> = { release: 0, beta: 1, alpha: 2 };

/**
 * The version an install without an explicit choice would take, mirroring the
 * backend: release over beta over alpha, the server's own loader first, then
 * newest (the list already arrives newest first).
 */
export function pickBest(versions: AddonVersion[], context?: AddonContext): AddonVersion | undefined {
  const own = context?.loaders[0];
  return versions
    .filter((version) => version.compatible)
    .map((version, index) => ({ version, index }))
    .sort((a, b) => {
      const type = (TYPE_RANK[a.version.version_type] ?? 3) - (TYPE_RANK[b.version.version_type] ?? 3);
      if (type !== 0) return type;
      const loader =
        Number(!own || !a.version.loaders.includes(own)) - Number(!own || !b.version.loaders.includes(own));
      if (loader !== 0) return loader;
      return a.index - b.index;
    })[0]?.version;
}

/** "3 days ago", in the app language. */
export function relativeDate(iso: string | null | undefined, language: string): string | null {
  if (!iso) return null;
  const then = new Date(iso).getTime();
  if (Number.isNaN(then)) return null;
  const seconds = Math.round((then - Date.now()) / 1000);
  const format = new Intl.RelativeTimeFormat(language, { numeric: "auto" });
  const steps: [Intl.RelativeTimeFormatUnit, number][] = [
    ["year", 31_536_000],
    ["month", 2_592_000],
    ["week", 604_800],
    ["day", 86_400],
    ["hour", 3_600],
    ["minute", 60],
  ];
  for (const [unit, size] of steps) {
    if (Math.abs(seconds) >= size) return format.format(Math.round(seconds / size), unit);
  }
  return format.format(0, "minute");
}

/** Compact count: 1.2M, 34K. */
export function compactCount(value: number, language: string): string {
  return new Intl.NumberFormat(language, { notation: "compact", maximumFractionDigits: 1 }).format(value);
}

/** Loader tags read as names: `neoforge` → `NeoForge`. */
const LOADER_NAMES: Record<string, string> = {
  fabric: "Fabric",
  quilt: "Quilt",
  forge: "Forge",
  neoforge: "NeoForge",
  paper: "Paper",
  purpur: "Purpur",
  spigot: "Spigot",
  bukkit: "Bukkit",
  folia: "Folia",
  velocity: "Velocity",
  bungeecord: "BungeeCord",
  waterfall: "Waterfall",
  sponge: "Sponge",
};

export function loaderName(tag: string): string {
  return LOADER_NAMES[tag] ?? tag.charAt(0).toUpperCase() + tag.slice(1);
}

/** Modrinth's public page for a project. */
export function modrinthUrl(projectType: string, slug: string): string {
  const type = projectType === "plugin" || projectType === "mod" ? projectType : "project";
  return `https://modrinth.com/${type}/${encodeURIComponent(slug)}`;
}

export async function openExternal(url: string) {
  try {
    const { openUrl } = await import("@tauri-apps/plugin-opener");
    await openUrl(url);
  } catch {
    // Nothing useful to say: the link is a convenience.
  }
}

/** The server's own loader as a name, or "" when it has none. */
export function primaryLoader(context: AddonContext): string {
  const own = context.loaders[0];
  return own ? loaderName(own) : "";
}

/** What is queued for one jar on disk, for its badge in the installed list. */
export function pendingFor(pending: PendingAddonChange[], fileName: string): PendingAddonChange | undefined {
  return pending.find((change) =>
    change.action === "install" ? change.replace === fileName : change.file_name === fileName,
  );
}

/** Project ids with an install waiting for the server to stop. */
export function pendingProjects(pending: PendingAddonChange[] | undefined): Set<string> {
  const ids = new Set<string>();
  for (const change of pending ?? []) {
    if (change.action === "install" && change.project_id) ids.add(change.project_id);
  }
  return ids;
}
