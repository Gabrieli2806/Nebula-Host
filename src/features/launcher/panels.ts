/**
 * Where the launcher is, kept in the URL's query on the one `/launcher` route:
 *
 *   /launcher                                   the library
 *   /launcher?tab=discover|screenshots|skin     another section
 *   /launcher?instance=<id>&panel=<panel>       one instance, one of its panels
 *
 * In the URL, not in component state, so the back button (Android's
 * included) walks back through panels and instances to the library instead
 * of leaving the launcher, a home-screen shortcut can open an instance, and
 * the page survives a reload.
 */

import type { IconName } from "@/ui";
import type { ContentKind } from "./types";

export type LauncherSection = "library" | "discover" | "screenshots" | "skin";

export const LAUNCHER_SECTIONS: { id: LauncherSection; icon: IconName; labelKey: string }[] = [
  { id: "library", icon: "servers", labelKey: "launcher.library" },
  { id: "discover", icon: "search", labelKey: "launcher.modpacks" },
  { id: "screenshots", icon: "image", labelKey: "launcher.screenshots" },
  { id: "skin", icon: "users", labelKey: "launcher.skin" },
];

export type InstancePanel = "content" | "add" | "log" | "settings";

export const INSTANCE_PANELS: { id: InstancePanel; icon: IconName; labelKey: string }[] = [
  { id: "content", icon: "puzzle", labelKey: "launcher.content" },
  { id: "add", icon: "plus", labelKey: "launcher.addContent" },
  { id: "log", icon: "terminal", labelKey: "launcher.gameLogs" },
  { id: "settings", icon: "sliders", labelKey: "launcher.instanceSettings" },
];

export type AddKind = Exclude<ContentKind, "modpack">;

export function isSection(value: string | null | undefined): value is LauncherSection {
  return LAUNCHER_SECTIONS.some((section) => section.id === value);
}

export function isInstancePanel(value: string | null | undefined): value is InstancePanel {
  return INSTANCE_PANELS.some((panel) => panel.id === value);
}

export function isAddKind(value: string | null | undefined): value is AddKind {
  return value === "mod" || value === "resourcepack" || value === "shader";
}

export function sectionUrl(section: LauncherSection): string {
  return section === "library" ? "/launcher" : `/launcher?tab=${section}`;
}

export function instanceUrl(profileId: string, panel: InstancePanel = "content", kind?: AddKind): string {
  const query = new URLSearchParams({ instance: profileId });
  if (panel !== "content") query.set("panel", panel);
  if (panel === "add" && kind) query.set("kind", kind);
  return `/launcher?${query.toString()}`;
}
