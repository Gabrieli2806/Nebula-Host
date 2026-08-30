/**
 * The shapes the Rust side sends.
 *
 * These mirror `src-tauri/src/model.rs` and `game/manifest.rs`. They are hand
 * written rather than generated so the UI can be explicit about what it relies
 * on — but they must stay in step: a rename on either side should fail
 * `npm run typecheck` here, not surprise a user at runtime.
 */

export type InstanceStatus =
  | "offline"
  | "sleeping"
  | "starting"
  | "online"
  | "stopping"
  | "crashed";

export type RuntimeKind = "host_process" | "docker";

export interface BuildSelector {
  distribution: string;
  version: string;
  loader_version?: string | null;
}

export interface ScheduleRule {
  id: string;
  enabled: boolean;
  action: "start" | "stop" | "restart" | "backup";
  /** Local time, `HH:MM`. */
  time: string;
  /** 0 = Sunday … 6 = Saturday. */
  days: number[];
}

export interface Instance {
  id: string;
  node_id: string;
  game: string;
  name: string;
  build: BuildSelector;
  track_latest_version: boolean;
  path: string;
  runtime_kind: RuntimeKind;
  resources: { memory_mb: number; runtime_path?: string | null; extra_args: string[] };
  automation: {
    sleep_when_empty: boolean;
    sleep_after_minutes: number;
    wake_on_join: boolean;
    restart_after_crash: boolean;
    restart_attempts_remaining: number;
    schedules: ScheduleRule[];
  };
  ports: Record<string, number>;
  install?: { entry_point?: string; java_major?: number } | null;
  icon?: string | null;
  created_at: string;
  last_started_at?: string | null;
}

/** Config and live state arrive flattened together. */
export interface InstanceView extends Instance {
  status: InstanceStatus;
  players_online: number;
  players_max: number;
  cpu_percent: number;
  memory_used_mb: number;
  uptime_seconds?: number | null;
  tick_rate?: number | null;
  last_player_seen_at?: string | null;
  address?: string | null;
}

export type ConsoleLevel = "info" | "warn" | "error" | "system";

export interface ConsoleLine {
  timestamp: string;
  level: ConsoleLevel;
  message: string;
}

// -- game manifests ---------------------------------------------------------

export interface Distribution {
  id: string;
  name: string;
  summary_key: string;
  has_loader_version: boolean;
  addon_loaders?: string[];
  addon_dir?: string | null;
  tick_rate_command?: string | null;
}

export type SettingKind =
  | { type: "bool"; default: boolean }
  | { type: "int"; default: number; min?: number; max?: number; unit?: string }
  | { type: "text"; default: string; max_len?: number }
  | { type: "choice"; default: string; options: { value: string; label_key: string }[] };

interface SettingBase {
  key: string;
  label_key: string;
  help_key?: string;
  group: string;
  restart_required: boolean;
}

/** Distributes over the union so `setting.type` still narrows. */
type WithBase<T> = T extends unknown ? T & SettingBase : never;

export type SettingField = WithBase<SettingKind>;

export interface GameManifest {
  id: string;
  name: string;
  accent: string;
  icon: string;
  distributions: Distribution[];
  runtime: { kind: "java"; min_major: number } | { kind: "native" } | { kind: "container" };
  ports: { key: string; default: number; protocol: string; primary: boolean }[];
  settings: SettingField[];
  wake?: { protocol: string; sleeping_motd: string } | null;
}

export interface VersionInfo {
  id: string;
  channel: string;
  stable: boolean;
  released_at?: string | null;
}

// -- node -------------------------------------------------------------------

export interface Node {
  id: string;
  name: string;
  transport: { kind: "local" } | { kind: "paired"; address: string; fingerprint: string };
  paired_devices: { id: string; name: string; paired_at: string }[];
  created_at: string;
}

export interface NodeHealth {
  cpu_percent: number;
  memory_used_mb: number;
  memory_total_mb: number;
  disk_used_gb: number;
  disk_total_gb: number;
  lan_address?: string | null;
  instances_running: number;
}

export interface Requirement {
  id: string;
  status: "ready" | "optional" | "missing";
  found?: string | null;
}

// -- files, backups, players, addons ---------------------------------------

export interface FileEntry {
  name: string;
  path: string;
  isDir: boolean;
  size: number;
  modified?: string | null;
}

export interface Backup {
  id: string;
  instanceId: string;
  label?: string | null;
  createdAt: string;
  sizeBytes: number;
  path: string;
}

export interface PlayerEntry {
  name: string;
  uuid?: string | null;
  level?: number | null;
  reason?: string | null;
  expires?: string | null;
}

export type PlayerLists = Record<string, PlayerEntry[]>;

export interface AddonCatalogVersion {
  id: string;
  name: string;
  version_number: string;
  download_url: string;
  file_name: string;
  file_size: number;
}

export interface AddonCatalogEntry {
  project_id: string;
  slug: string;
  title: string;
  description: string;
  icon_url?: string | null;
  downloads: number;
  project_type: string;
  featured_version: AddonCatalogVersion;
}

export interface AddonSearchResult {
  entries: AddonCatalogEntry[];
  total_hits: number;
}

export interface InstalledAddon {
  id: string;
  name: string;
  version: string;
  enabled: boolean;
  file_name: string;
}

export interface AddonMeta {
  file_name: string;
  project_id: string;
  title: string;
  description: string;
  icon_url?: string | null;
  installed_version_number: string;
  latest_version_number?: string | null;
  latest_download_url?: string | null;
  latest_file_name?: string | null;
  update_available: boolean;
}

export interface AppSettings {
  language: string;
  /** Accent id the UI themes itself with: `cyan`, `amber`, `emerald`, … */
  accent: string;
  density: string;
  instancesDir?: string | null;
  javaPaths: Record<string, string>;
  javaPath?: string | null;
  launchAtStartup: boolean;
  runInTray: boolean;
  allowUpnp: boolean;
  listenOnLan: boolean;
}

export type SettingValue = SettingField extends unknown
  ? SettingField & { value: string }
  : never;

// -- events -----------------------------------------------------------------

export type InstallProgress =
  | { phase: "resolving" }
  | { phase: "downloading"; file: string; received: number; total?: number }
  | { phase: "installing_loader"; distribution: string }
  | { phase: "configuring" }
  | { phase: "generating_world" }
  | { phase: "ready" }
  | { phase: "failed"; code: string };

export type AppEvent =
  | { type: "status"; instanceId: string; status: InstanceStatus }
  | { type: "console"; instanceId: string; line: ConsoleLine }
  | { type: "install"; instanceId: string; progress: InstallProgress }
  | { type: "stats"; instanceId: string; state: Omit<InstanceView, keyof Instance> }
  | { type: "players"; instanceId: string; online: string[] };
