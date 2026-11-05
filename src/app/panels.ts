/**
 * The panels a server has, in the order they are worked through. The sidebar
 * (AppShell), the phone tab bar (MobileTabs) and the server page all read this
 * one list, so a panel is added in one place.
 */

import type { IconName } from "@/ui";

export const SERVER_PANELS: {
  id: string;
  icon: IconName;
  labelKey: string;
  group: "operate" | "manage";
}[] = [
  { id: "overview", icon: "servers", labelKey: "overview.title", group: "operate" },
  { id: "console", icon: "terminal", labelKey: "console.title", group: "operate" },
  { id: "files", icon: "folder", labelKey: "files.title", group: "operate" },
  { id: "players", icon: "users", labelKey: "players.title", group: "manage" },
  { id: "addons", icon: "puzzle", labelKey: "addons.title", group: "manage" },
  { id: "backups", icon: "archive", labelKey: "backups.title", group: "manage" },
  { id: "settings", icon: "sliders", labelKey: "settings.title", group: "manage" },
];
