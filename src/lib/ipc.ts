/**
 * The one place the UI talks to the backend.
 *
 * Two jobs: give every command a typed signature, and turn whatever comes back
 * from a rejection into an `AppError` the error surface can translate. No
 * component calls `invoke` directly.
 */

import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWebview } from "@tauri-apps/api/webview";

import { activeRemote, remoteCall, remoteEvents, uploadFromBrowser, type RemoteNode } from "@/lib/connection";
import { IS_WEB, setWebPrefs, webPrefs } from "@/lib/web";
import type { ContentKind, CreateProfile, DeviceLogin, GameVersion, LauncherAccount, LauncherCapabilities, LauncherProfile, LauncherSearch, LoginResult } from "@/features/launcher/types";

// Client accounts and profiles always belong to this device, even while the
// host UI is connected to another node. Never send credentials to remoteCall.
async function launcherCall<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  if (IS_WEB) throw { code: "local_only", params: {} };
  return invoke<T>(command, args);
}
export const launcherApi = {
  capabilities: () => launcherCall<LauncherCapabilities>("launcher_capabilities"),
  profiles: () => launcherCall<LauncherProfile[]>("launcher_profiles"),
  create: (input: CreateProfile) => launcherCall<LauncherProfile>("launcher_create", { input }),
  remove: (id: string) => launcherCall<void>("launcher_remove", { id }),
  versions: () => launcherCall<GameVersion[]>("launcher_versions"),
  search: (query: string, kind: ContentKind, offset: number) => launcherCall<LauncherSearch>("launcher_search", { query, kind, offset }),
  accounts: () => launcherCall<LauncherAccount[]>("launcher_accounts"),
  begin: (clientId?: string) => launcherCall<DeviceLogin>("launcher_login_begin", { clientId }),
  poll: (id: string) => launcherCall<LoginResult>("launcher_login_poll", { id }),
  cancel: (id: string) => launcherCall<void>("launcher_login_cancel", { id }),
  logout: (id: string) => launcherCall<void>("launcher_logout", { id }),
};

import type {
  AddonBrowseRequest,
  AddonBrowseResult,
  AddonContext,
  AddonDependency,
  AddonInstallResult,
  AddonMeta,
  AddonSearchResult,
  AddonVersion,
  CompletionSet,
  ConflictPolicy,
  DiskUsage,
  ImportCheck,
  ImportSummary,
  InstalledAddonsReport,
  JavaInstallation,
  JavaRuntimes,
  LogFiles,
  LogSource,
  PendingAddonChange,
  PlayerIdentities,
  SharedLog,
  WindowDragDrop,
  AppEvent,
  AppSettings,
  Backup,
  ConsoleLine,
  McpStatus,
  WorldImportResult,
  JavaOverview,
  AppLog,
  RemoteStatus,
  UpnpCheck,
  StorageLocation,
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
  TransferOutcome,
  TransferRequest,
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

/** Commands that always run on this PC, whichever machine is being managed. */
const ALWAYS_LOCAL = new Set([
  "mcp_status",
  "mcp_configure",
  "mcp_regenerate_token",
  "remote_status",
  "remote_configure",
]);

/** Settings that describe this window (look and language), never the node. */
const UI_PREFS = ["language", "accent", "density"] as const;

async function local<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  if (IS_WEB) return webLocal<T>(command, args);
  return invoke<T>(command, args);
}

/** The web panel has no backend of its own: how it looks stays in this
 *  browser, and everything else goes to the machine that served it. */
async function webLocal<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  if (command === "settings_get") return webPrefs() as T;
  if (command === "settings_set") {
    const next = (args?.settings ?? {}) as Record<string, unknown>;
    const prefs: Record<string, unknown> = {};
    for (const key of UI_PREFS) prefs[key] = next[key];
    setWebPrefs(prefs);
    return prefs as T;
  }
  const node = activeRemote();
  if (!node || command === "app_open_log_folder" || command === "upload_to_node") {
    throw { code: "local_only", params: {} };
  }
  return remoteCall<T>(node, command, args);
}

const WEB_UPLOAD_EVENT = "nebula:web-upload";

/** Tauri events, or nothing in the web panel (which has no Tauri). */
function tauriListen<T>(event: string, handler: (payload: T) => void): () => void {
  if (IS_WEB) return () => {};
  const unlisten = listen<T>(event, (message) => handler(message.payload));
  return () => {
    void unlisten.then((stop) => stop()).catch(() => {});
  };
}

async function routed<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  const node = activeRemote();
  if (!node || ALWAYS_LOCAL.has(command)) return local<T>(command, args);

  // Settings are split: the node's own (folders, Java, network) come from the
  // node; how this window looks stays this desktop's.
  if (command === "settings_get") {
    const [remote, mine] = await Promise.all([
      remoteCall<AppSettings>(node, "settings_get"),
      local<AppSettings>("settings_get"),
    ]);
    const merged = { ...remote } as AppSettings;
    for (const key of UI_PREFS) {
      // A browser that never chose keeps the machine's look.
      if (mine[key] !== undefined) (merged as unknown as Record<string, unknown>)[key] = mine[key];
    }
    return merged as T;
  }
  if (command === "settings_set") {
    const next = (args?.settings ?? {}) as AppSettings;
    const mine = await local<AppSettings>("settings_get");
    const ui = { ...mine } as AppSettings;
    for (const key of UI_PREFS) (ui as unknown as Record<string, unknown>)[key] = next[key];
    await local("settings_set", { settings: ui });
    const saved = await remoteCall<AppSettings>(node, "settings_set", { settings: next });
    for (const key of UI_PREFS) (saved as unknown as Record<string, unknown>)[key] = next[key];
    return saved as T;
  }
  return remoteCall<T>(node, command, args);
}

/**
 * Ask a specific machine, whichever one this window manages: `null` is this
 * PC. For screens that look at every machine at once (Machines).
 */
export async function callMachine<T>(node: RemoteNode | null, command: string, args?: Record<string, unknown>): Promise<T> {
  try {
    return node ? await remoteCall<T>(node, command, args) : await local<T>(command, args);
  } catch (error) {
    throw toAppError(error);
  }
}

async function call<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  try {
    return await routed<T>(command, args);
  } catch (error) {
    throw toAppError(error);
  }
}

export const api = {
  node: {
    get: () => call<Node>("node_get"),
    /** This PC's own node record, whichever machine the window manages. */
    getLocal: () => local<Node>("node_get").catch((error) => {
      throw toAppError(error);
    }),
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
      /** The user ticked "I accept" for the game's agreement (EULA). */
      acceptAgreement?: boolean;
    }) => call<Instance>("instance_create", { request }),
    /** Record that the user accepted the game's agreement for this server. */
    acceptAgreement: (instanceId: string) => call<void>("instance_accept_agreement", { instanceId }),
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
    /**
     * Console autocomplete. When the server is online and nothing is cached,
     * this silently runs `help` on it (hidden from the console) — allow ~1–6 s
     * on the first call. Cached until restart / addon change; `refresh` refetches.
     */
    completions: (instanceId: string, refresh = false) =>
      call<CompletionSet>("console_completions", { instanceId, refresh }),
    /** Bytes used by the instance folder; cached 60 s unless `refresh`. */
    diskUsage: (instanceId: string, refresh = false) =>
      call<DiskUsage>("instance_disk_usage", { instanceId, refresh }),
    update: (instanceId: string, patch: InstancePatch) =>
      call<Instance>("instance_update", { instanceId, patch }),
    forwardPort: (instanceId: string) => call<void>("instance_forward_port", { instanceId }),
    releasePort: (instanceId: string) => call<void>("instance_release_port", { instanceId }),
    /**
     * Make a picture the server's icon. `data` is base64 or a `data:` URL; the
     * backend squares it (`cover` crops the centre, `contain` letterboxes)
     * and writes the game's icon file.
     */
    setIcon: (instanceId: string, data: string, fit: "cover" | "contain" = "cover") =>
      call<void>("instance_icon_set", { instanceId, data, fit }),
    clearIcon: (instanceId: string) => call<void>("instance_icon_clear", { instanceId }),
    /** The game's own log files: latest log and recent crash reports. */
    logFiles: (instanceId: string) => call<LogFiles>("instance_log_files", { instanceId }),
    /** Upload a log to mclo.gs. Ask the user first: logs carry names and IPs. */
    shareLog: (instanceId: string, source: LogSource) =>
      call<SharedLog>("instance_log_share", { instanceId, source }),
    /**
     * Play an older copy a transfer left behind anyway. It forks: from then on
     * this copy and the one on the other machine are different servers.
     */
    forcePlay: (instanceId: string) => call<Instance>("instance_force_play", { instanceId }),
    /** Hide the "this copy split from the one on X" notice for good. */
    dismissFork: (instanceId: string) => call<Instance>("instance_dismiss_fork", { instanceId }),
  },

  transfer: {
    /**
     * Move a server between machines. Always runs on this PC's backend, which
     * may be the source, the target or a relay between two nodes; the bytes
     * never pass through the webview. Progress arrives as `transfer` events on
     * this PC's channel (see `onLocalAppEvent`), and on the source's while it
     * packs.
     */
    run: (request: TransferRequest) =>
      local<TransferOutcome>("transfer_run", { request }).catch((error) => {
        throw toAppError(error);
      }),
    /** Call off a running transfer; the server stays where it was. */
    cancel: (transferId: string) => local<boolean>("transfer_cancel", { transferId }),
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
    /** Open the instance folder in this PC's file manager (local servers only). */
    openFolder: (instanceId: string) => local<void>("instance_open_folder", { instanceId }).catch((error) => {
      throw toAppError(error);
    }),
    /** Dry run of a drop into `target` (instance-relative, "" = root). */
    importCheck: (instanceId: string, target: string, sources: string[]) =>
      call<ImportCheck>("files_import_check", { instanceId, target, sources }),
    /**
     * Copy dropped OS paths (files and folders, recursive) into `target`.
     * Progress arrives as `{ type: "import" }` events carrying `importId`.
     */
    import: (
      instanceId: string,
      target: string,
      sources: string[],
      conflict: ConflictPolicy = "keep_both",
      importId?: string,
    ) => call<ImportSummary>("files_import", { instanceId, target, sources, conflict, importId }),
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
    /** Name → UUID from the server's lists, usercache.json and join lines. */
    identities: (instanceId: string) =>
      call<PlayerIdentities>("players_identities", { instanceId }),
  },

  addons: {
    // -- round 3 (use these) ------------------------------------------------
    context: (instanceId: string) => call<AddonContext>("addons_context", { instanceId }),
    /** Every jar, hash-identified on Modrinth, with update info. */
    list: (instanceId: string) => call<InstalledAddonsReport>("addons_list", { instanceId }),
    browse: (instanceId: string, request: AddonBrowseRequest) =>
      call<AddonBrowseResult>("addons_browse", { instanceId, request }),
    /** Compatible versions, newest first; `all` lifts the loader/game filter. */
    versions: (instanceId: string, projectId: string, all = false) =>
      call<AddonVersion[]>("addon_versions", { instanceId, projectId, all }),
    dependencies: (instanceId: string, versionId: string) =>
      call<AddonDependency[]>("addon_dependencies", { instanceId, versionId }),
    /**
     * Install a project: `versionId`, or the best compatible version when
     * omitted. Replaces an existing jar of the same project (or `replace`).
     */
    installVersion: (instanceId: string, projectId: string, versionId?: string, replace?: string) =>
      call<AddonInstallResult>("addon_install_version", { instanceId, projectId, versionId, replace }),
    /** Update one jar to its latest compatible version, in place. */
    update: (instanceId: string, fileName: string) =>
      call<AddonInstallResult>("addon_update", { instanceId, fileName }),
    /** Changes queued while the server runs; applied when it stops. */
    pending: (instanceId: string) => call<PendingAddonChange[]>("addons_pending", { instanceId }),
    cancelPending: (instanceId: string, changeId: string) =>
      call<PendingAddonChange[]>("addon_pending_cancel", { instanceId, changeId }),
    /** Retry the queue now; a no-op while the server runs. */
    applyPending: (instanceId: string) =>
      call<PendingAddonChange[]>("addons_pending_apply", { instanceId }),
    // -- round 2 (deprecated; kept until AddonsPanel migrates) -------------
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

  java: {
    /** Every JVM on the node and which one this server's version wants. */
    runtimes: (instanceId: string, refresh = false) =>
      call<JavaRuntimes>("java_runtimes", { instanceId, refresh }),
    /** Download Eclipse Temurin; progress arrives as `javaDownload` events. */
    download: (major: number) => call<JavaInstallation>("java_download", { major }),
    /** Settings › Java: the four slots (25/21/17/8) and every JVM found. */
    overview: (refresh = false) => call<JavaOverview>("java_overview", { refresh }),
  },

  worlds: {
    /**
     * Import a world from a .zip or folder on this PC. When managing a remote
     * machine the file is streamed there first (by this PC's backend, not the
     * webview) and imported from the upload.
     */
    import: async (instanceId: string, path: string, skipBackup = false): Promise<WorldImportResult> => {
      const node = activeRemote();
      try {
        if (!node) return await invoke<WorldImportResult>("world_import", { instanceId, path, skipBackup });
        const uploadId = await invoke<string>("upload_to_node", { address: node.address, token: node.token, path });
        return await remoteCall<WorldImportResult>(node, "world_import", { instanceId, uploadId, skipBackup });
      } catch (error) {
        throw toAppError(error);
      }
    },
    /** Web panel: import a .zip chosen in the browser, streamed from it. */
    importFile: async (instanceId: string, file: File, skipBackup = false): Promise<WorldImportResult> => {
      const node = activeRemote();
      try {
        if (!node) throw { code: "local_only", params: {} };
        // Reported like the desktop's own upload (`onLocalUpload`).
        const uploadId = await uploadFromBrowser(node, file, (sent, total) =>
          window.dispatchEvent(new CustomEvent(WEB_UPLOAD_EVENT, { detail: { sent, total } })),
        );
        return await remoteCall<WorldImportResult>(node, "world_import", { instanceId, uploadId, skipBackup });
      } catch (error) {
        throw toAppError(error);
      }
    },
  },

  storage: {
    /** Folders the phone app can keep servers in (empty on the desktop). */
    locations: () => call<StorageLocation[]>("storage_locations"),
  },

  app: {
    /** Tail of the app's own log (the managed machine's, when remote). */
    log: (lines = 400) => call<AppLog>("app_log", { lines }),
    /** Open the logs folder on this PC in the file manager. */
    openLogFolder: () => local<void>("app_open_log_folder").catch((error) => {
      throw toAppError(error);
    }),
  },

  mcp: {
    status: () => call<McpStatus>("mcp_status"),
    configure: (enabled: boolean, readOnly: boolean, port?: number) =>
      call<McpStatus>("mcp_configure", { enabled, readOnly, port }),
    regenerateToken: () => call<McpStatus>("mcp_regenerate_token"),
  },

  remote: {
    status: () => call<RemoteStatus>("remote_status"),
    configure: (enabled: boolean, webPanel?: boolean) =>
      call<RemoteStatus>("remote_configure", { enabled, webPanel }),
    forgetDevice: (deviceId: string) => call<Node>("node_forget_device", { deviceId }),
  },

  upnp: {
    check: () => call<UpnpCheck>("upnp_check"),
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
  autoBackupHours?: number;
  autoBackupKeep?: number;
  trackLatestVersion?: boolean;
  schedules?: ScheduleRule[];
}

/**
 * Subscribe to native (OS) file drags over the window — the only drag source
 * that yields real paths (Tauri `dragDropEnabled: true`; HTML5 drop events do
 * not fire for OS files in that mode). Positions are CSS pixels relative to
 * the viewport, ready for `document.elementFromPoint`. Returns an unsubscribe
 * function; safe to call from an effect.
 */
export function onWindowDragDrop(handler: (event: WindowDragDrop) => void): () => void {
  if (IS_WEB) return () => {};
  let disposed = false;
  let stop: (() => void) | null = null;
  const toCss = (position: { x: number; y: number }) => {
    const ratio = window.devicePixelRatio || 1;
    return { x: position.x / ratio, y: position.y / ratio };
  };
  void getCurrentWebview()
    .onDragDropEvent(({ payload }) => {
      switch (payload.type) {
        case "enter":
          handler({ type: "enter", paths: payload.paths, position: toCss(payload.position) });
          break;
        case "over":
          handler({ type: "over", position: toCss(payload.position) });
          break;
        case "drop":
          handler({ type: "drop", paths: payload.paths, position: toCss(payload.position) });
          break;
        default:
          handler({ type: "leave" });
      }
    })
    .then((unlisten) => {
      if (disposed) unlisten();
      else stop = unlisten;
    })
    .catch(() => {});
  return () => {
    disposed = true;
    stop?.();
  };
}

/** Events from this PC's backend, whichever machine the window manages. */
export function onLocalAppEvent(handler: (event: AppEvent) => void): () => void {
  return tauriListen<AppEvent>("nebula:event", handler);
}

/** A start from the tray menu (this PC) that needs the window. */
export interface TrayRefusal {
  instanceId: string;
  reason: "agreement_required" | "transferred_out";
  url?: string | null;
}

/** The tray asked for something only the window can finish. */
export function onTrayRefused(handler: (refusal: TrayRefusal) => void): () => void {
  return tauriListen<TrayRefusal>("nebula:tray-refused", handler);
}

/** Bytes this PC has sent while streaming a file to a remote machine
 *  (always a local event, even while managing that machine). */
export function onLocalUpload(handler: (progress: { sent: number; total: number }) => void): () => void {
  if (IS_WEB) {
    const listener = (event: Event) => handler((event as CustomEvent<{ sent: number; total: number }>).detail);
    window.addEventListener(WEB_UPLOAD_EVENT, listener);
    return () => window.removeEventListener(WEB_UPLOAD_EVENT, listener);
  }
  return tauriListen<{ sent: number; total: number }>("nebula:upload", handler);
}

/** Subscribe to backend events. Returns an unsubscribe function. */
export function onAppEvent(handler: (event: AppEvent) => void): () => void {
  const node = activeRemote();
  if (node) return remoteEvents(node, (payload) => handler(payload as AppEvent));
  return tauriListen<AppEvent>("nebula:event", handler);
}
