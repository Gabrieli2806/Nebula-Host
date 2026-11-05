/**
 * Where things live in the launcher. Like a server, the open section and an
 * open instance's panel are in the URL, so the sidebar and the phone tab bar
 * are plain links and the back button (Android's included) walks through them:
 *
 *   /launcher                              the library
 *   /launcher/:section                     modpacks, screenshots, skin
 *   /launcher/instances/:profileId/:panel  one instance, one of its panels
 */

import type { IconName } from "@/ui";

export type LauncherSection = "library" | "discover" | "screenshots" | "skin";

export const LAUNCHER_SECTIONS: { id: LauncherSection; icon: IconName; labelKey: string }[] = [
  { id: "library", icon: "servers", labelKey: "launcher.library" },
  { id: "discover", icon: "search", labelKey: "launcher.modpacks" },
  { id: "screenshots", icon: "image", labelKey: "launcher.screenshots" },
  { id: "skin", icon: "users", labelKey: "launcher.skin" },
];

export type InstancePanel = "content" | "add" | "log" | "settings";

/** An open instance's panels. `shortKey` is the phone tab bar's label. */
export const INSTANCE_PANELS: { id: InstancePanel; icon: IconName; labelKey: string; shortKey: string }[] = [
  { id: "content", icon: "puzzle", labelKey: "launcher.content", shortKey: "launcher.content" },
  { id: "add", icon: "plus", labelKey: "launcher.addContent", shortKey: "launcher.tabAdd" },
  { id: "log", icon: "terminal", labelKey: "launcher.gameLogs", shortKey: "launcher.tabLog" },
  { id: "settings", icon: "sliders", labelKey: "launcher.instanceSettings", shortKey: "launcher.instanceSettings" },
];

export function sectionPath(section: LauncherSection): string {
  return section === "library" ? "/launcher" : `/launcher/${section}`;
}

export function instancePath(profileId: string, panel: InstancePanel = "content"): string {
  return `/launcher/instances/${encodeURIComponent(profileId)}/${panel}`;
}

export function isSection(value: string | undefined): value is LauncherSection {
  return LAUNCHER_SECTIONS.some((section) => section.id === value);
}

export function isInstancePanel(value: string | undefined): value is InstancePanel {
  return INSTANCE_PANELS.some((panel) => panel.id === value);
}
