/**
 * The right-click menu of the servers dashboard: on a server, the actions
 * that fit its state; on empty space, "Create server" and "Refresh".
 */

import type { MouseEvent } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { useNavigate } from "react-router-dom";

import { keys, useInstanceActions } from "@/hooks/useBackend";
import { useI18n } from "@/i18n";
import { isRemote } from "@/lib/connection";
import { api } from "@/lib/ipc";
import type { InstanceView } from "@/types/api";
import { useContextMenu, type MenuItem } from "@/ui/ContextMenu";

export function useServerMenu(instances: InstanceView[], onCreate: () => void) {
  const { t } = useI18n();
  const navigate = useNavigate();
  const client = useQueryClient();
  const actions = useInstanceActions();
  const { open, menu } = useContextMenu();

  const serverItems = (instance: InstanceView): MenuItem[] => {
    const go = (panel: string) => navigate(`/servers/${instance.id}/${panel}`);
    const running = instance.status === "online" || instance.status === "starting";
    const busy = instance.status === "stopping" || instance.status === "starting";
    const items: MenuItem[] = [
      { label: t("menu.open"), icon: "servers", onSelect: () => go("overview") },
      { kind: "separator" },
    ];
    if (running) {
      items.push(
        { label: t("actions.stop"), icon: "stop", disabled: busy, onSelect: () => actions.stop.mutate(instance.id) },
        { label: t("actions.restart"), icon: "restart", disabled: busy, onSelect: () => actions.restart.mutate(instance.id) },
      );
    } else {
      items.push({
        label: instance.status === "sleeping" ? t("actions.wake") : t("actions.start"),
        icon: "play",
        disabled: busy,
        onSelect: () => actions.start.mutate(instance.id),
      });
    }
    items.push(
      { kind: "separator" },
      { label: t("menu.console"), icon: "terminal", onSelect: () => go("console") },
      { label: t("menu.files"), icon: "folder", onSelect: () => go("files") },
      { label: t("menu.backups"), icon: "archive", onSelect: () => go("backups") },
      { label: t("menu.settings"), icon: "sliders", onSelect: () => go("settings") },
    );
    if (instance.address) {
      const address = instance.address;
      items.push({ kind: "separator" }, { label: t("menu.copyAddress"), icon: "copy", onSelect: () => void navigator.clipboard.writeText(address) });
    }
    if (!isRemote()) {
      items.push({
        label: t("actions.openFolder"),
        icon: "external",
        onSelect: () => void api.files.openFolder(instance.id).catch(() => undefined),
      });
    }
    return items;
  };

  const backgroundItems = (): MenuItem[] => [
    { label: t("actions.create"), icon: "plus", onSelect: onCreate },
    { label: t("menu.refresh"), icon: "restart", onSelect: () => void client.invalidateQueries({ queryKey: keys.instances }) },
  ];

  /** For the dashboard root: finds the server under the pointer, if any. */
  const onContextMenu = (event: MouseEvent) => {
    const target = event.target as HTMLElement;
    // Let text fields keep the native cut/copy/paste menu.
    if (target.closest("input, textarea")) return;
    const id = target.closest<HTMLElement>("[data-instance-id]")?.dataset.instanceId;
    const instance = id ? instances.find((candidate) => candidate.id === id) : undefined;
    open(event, instance ? serverItems(instance) : backgroundItems());
  };

  return { onContextMenu, menu };
}
