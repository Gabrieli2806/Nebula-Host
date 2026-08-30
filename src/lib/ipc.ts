/**
 * The one place the UI talks to the backend.
 *
 * Two jobs: give every command a typed signature, and turn whatever comes back
 * from a rejection into an `AppError` the error surface can translate. No
 * component calls `invoke` directly.
 */

import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

import type {
  AddonMeta,
  AddonSearchResult,
  AppEvent,
  AppSettings,
  Backup,
  ConsoleLine,
  FileEntry,
  GameManifest,
  InstalledAddon,
  Instance,
  InstanceView,
  Node,
  NodeHealth,
  PlayerEntry,
  PlayerLists,
  Requirement,
  RuntimeKind,
  ScheduleRule,
  SettingValue,
  VersionInfo,
} from "@/types/api";

/** A typed failure from the backend: a code, its parameters, and raw detail. */
export interface AppError {
  code: string;
  params: Record<string, string>;
  detail?: string;
}

export function isAppError(value: unknown): value is AppError {
  return typeof value === "object" && value !== null && typeof (value as AppError).code === "string";
}

/** Anything that is not a typed error still has to render as one. */
export function toAppError(value: unknown): AppError {
  if (isAppError(value)) return { ...value, params: value.params ?? {} };
  return { code: "unexpected", params: {}, detail: String(value) };
}

async function call<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  try {
    return await invoke<T>(command, args);
  } catch (error) {
    throw toAppError(error);
  }
}

export const api = {
  node: {
    get: () => call<Node>("node_get"),
    rename: (name: string) => call<Node>("node_rename", { name }),
    health: () => call<NodeHealth>("node_health"),
    requirements: () => call<Requirement[]>("node_requirements"),
    pairingCode: () => call<{ code: string; expires_at: string }>("node_pairing_code"),
  },

  games: {
    list: () => call<GameManifest[]>("games_list"),
    versions: (game: string, distribution: string) =>
      call<VersionInfo[]>("game_versions", { game, distribution }),
    loaderVersions: (game: string, distribution: string, version: string) =>
      call<VersionInfo[]>("game_loader_versions", { game, distribution, version }),
  },

  instances: {
    list: () => call<InstanceView[]>("instances_list"),
    get: (instanceId: string) => call<InstanceView>("instance_get", { instanceId }),
    create: (request: {
      game: string;
      name: string;
      build: { distribution: string; version: string; loader_version?: string | null };
      memoryMb?: number;
      port?: number;
      runtimeKind?: RuntimeKind;
    }) => call<Instance>("instance_create", { request }),
    install: (instanceId: string) => call<void>("instance_install", { instanceId }),
    start: (instanceId: string) => call<void>("instance_start", { instanceId }),
    stop: (instanceId: string) => call<void>("instance_stop", { instanceId }),
    restart: (instanceId: string) => call<void>("instance_restart", { instanceId }),
    sleep: (instanceId: string) => call<void>("instance_sleep", { instanceId }),
    remove: (instanceId: string, deleteFiles: boolean) =>
      call<void>("instance_delete", { instanceId, deleteFiles }),
    command: (instanceId: string, command: string) =>
      call<void>("instance_command", { instanceId, command }),
    console: (instanceId: string, limit?: number) =>
      call<ConsoleLine[]>("instance_console", { instanceId, limit }),
    update: (instanceId: string, patch: InstancePatch) =>
      call<Instance>("instance_update", { instanceId, patch }),
    forwardPort: (instanceId: string) => call<void>("instance_forward_port", { instanceId }),
    releasePort: (instanceId: string) => call<void>("instance_release_port", { instanceId }),
  },

  files: {
    list: (instanceId: string, path?: string) =>
      call<FileEntry[]>("files_list", { instanceId, path }),
    read: (instanceId: string, path: string) => call<string>("file_read", { instanceId, path }),
    write: (instanceId: string, path: string, contents: string) =>
      call<void>("file_write", { instanceId, path, contents }),
    remove: (instanceId: string, path: string) => call<void>("file_delete", { instanceId, path }),
    rename: (instanceId: string, path: string, newName: string) =>
      call<void>("file_rename", { instanceId, path, newName }),
    createFolder: (instanceId: string, path: string) =>
      call<void>("folder_create", { instanceId, path }),
    reveal: (instanceId: string) => call<string>("instance_reveal", { instanceId }),
  },

  backups: {
    list: (instanceId: string) => call<Backup[]>("backups_list", { instanceId }),
    create: (instanceId: string, label?: string) =>
      call<Backup>("backup_create", { instanceId, label }),
    restore: (instanceId: string, backupId: string) =>
      call<void>("backup_restore", { instanceId, backupId }),
    remove: (instanceId: string, backupId: string) =>
      call<void>("backup_delete", { instanceId, backupId }),
  },

  players: {
    online: (instanceId: string) => call<string[]>("players_online", { instanceId }),
    lists: (instanceId: string) => call<PlayerLists>("players_lists", { instanceId }),
    writeList: (instanceId: string, list: string, entries: PlayerEntry[]) =>
      call<void>("players_write_list", { instanceId, list, entries }),
  },

  addons: {
    search: (instanceId: string, query: string, offset = 0, limit = 20) =>
      call<AddonSearchResult>("addons_search", { instanceId, query, offset, limit }),
    installed: (instanceId: string) =>
      call<[InstalledAddon[], AddonMeta[]]>("addons_installed", { instanceId }),
    install: (instanceId: string, url: string, fileName: string) =>
      call<void>("addon_install", { instanceId, url, fileName }),
    setEnabled: (instanceId: string, fileName: string, enabled: boolean) =>
      call<string>("addon_set_enabled", { instanceId, fileName, enabled }),
    remove: (instanceId: string, fileName: string) =>
      call<void>("addon_remove", { instanceId, fileName }),
  },

  settings: {
    get: () => call<AppSettings>("settings_get"),
    set: (settings: AppSettings) => call<AppSettings>("settings_set", { settings }),
    instanceGet: (instanceId: string) =>
      call<SettingValue[]>("instance_settings_get", { instanceId }),
    instanceSet: (instanceId: string, values: Record<string, string>) =>
      call<void>("instance_settings_set", { instanceId, values }),
    probeJava: (path: string) =>
      call<{ path: string; major: number; version: string }>("java_probe", { path }),
  },
};

export interface InstancePatch {
  name?: string;
  memoryMb?: number;
  port?: number;
  runtimeKind?: RuntimeKind;
  runtimePath?: string;
  extraArgs?: string[];
  sleepWhenEmpty?: boolean;
  sleepAfterMinutes?: number;
  wakeOnJoin?: boolean;
  restartAfterCrash?: boolean;
  trackLatestVersion?: boolean;
  schedules?: ScheduleRule[];
}

/** Subscribe to backend events. Returns an unsubscribe function. */
export function onAppEvent(handler: (event: AppEvent) => void): () => void {
  const unlisten = listen<AppEvent>("nebula:event", (message) => handler(message.payload));
  return () => {
    void unlisten.then((stop) => stop());
  };
}
