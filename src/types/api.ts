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
    /** Automatic backup every N hours (0 = off). */
    auto_backup_hours: number;
    /** Automatic backups kept; the oldest is removed. */
    auto_backup_keep: number;
    restart_attempts_remaining: number;
    schedules: ScheduleRule[];
  };
  ports: Record<string, number>;
  install?: { entry_point?: string; java_major?: number } | null;
  icon?: string | null;
  created_at: string;
  last_started_at?: string | null;
  /** Lineage counter: bumped when the server changes machines or is forked. */
  generation: number;
  /** Whether this copy is the server, or an older copy a transfer left behind. */
  transfer: TransferState;
  /** Set when this copy was forced into play while another machine had a newer one. */
  forked_from?: ForkInfo | null;
}

export type TransferState =
  | { state: "active" }
  | {
      state: "transferred_out";
      to_node_id: string;
      to_node_name: string;
      at: string;
      generation: number;
    };

export interface ForkInfo {
  node_id: string;
  node_name: string;
  generation: number;
  at: string;
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
  /** Internet address while UPnP forwards the port. */
  public_address?: string | null;
  /** The game's server-list icon as a `data:` URL, when the server has one. */
  icon_url?: string | null;
}

export type ConsoleLevel = "info" | "warn" | "error" | "system";

export interface ConsoleLine {
  timestamp: string;
  level: ConsoleLevel;
  /** The raw line (colour codes stripped). */
  message: string;
  /**
   * The line without the game's own time/level prefix, for display; the
   * console gutter already shows the time. Absent when there was no prefix.
   */
  body?: string | null;
  /** Present when the line is something a player did (chat, join, leave). */
  speaker?: ConsoleSpeaker | null;
}

export type SpeakerKind = "chat" | "join" | "leave";

export interface ConsoleSpeaker {
  /** Player name as the game spells it. */
  name: string;
  kind: SpeakerKind;
  /** What they said; only for `chat`. */
  message?: string | null;
}

// -- game manifests ---------------------------------------------------------

export interface Distribution {
  id: string;
  name: string;
  /** Identity colour from the manifest, `#rrggbb`. */
  accent?: string | null;
  summary_key: string;
  has_loader_version: boolean;
  addon_loaders?: string[];
  addon_dir?: string | null;
  /** `mod` or `plugin`, for distributions that take addons. */
  addon_project_type?: string | null;
  tick_rate_command?: string | null;
  /** Which of the app's own glyphs (src/ui/loaders) stands for it. */
  glyph?: string | null;
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
  /** The game's server-list icon file, when it has one. */
  server_icon?: { file: string; size: number } | null;
  /** A licence the server's operator must accept before it runs (the
   *  Minecraft EULA). The user accepts it; the app never does. */
  agreement?: { url: string } | null;
  /** The game's own log files, when it writes any. */
  logs?: { latest: string; crash_dir?: string | null } | null;
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
  /** Seconds since the machine booted (missing from older nodes). */
  uptime_seconds?: number;
  os?: string;
  cpu_count?: number;
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

// -- addons (round 3: Modrinth, hash-identified) ---------------------------

export interface AddonContext {
  /** False for distributions that take no addons (Vanilla). */
  supported: boolean;
  project_type?: "mod" | "plugin" | string | null;
  /** Loader tags in priority order, e.g. `["purpur","paper","spigot","bukkit"]`. */
  loaders: string[];
  game_version: string;
  /** Folder relative to the instance root (`mods` / `plugins`). */
  addon_dir?: string | null;
}

export type AddonSort = "relevance" | "downloads" | "follows" | "newest" | "updated";

export interface AddonBrowseRequest {
  query?: string;
  /** Default `relevance`; an empty query with `relevance` is sent as `downloads`. */
  sort?: AddonSort;
  offset?: number;
  /** 1–100, default 20. */
  limit?: number;
  /** Extra Modrinth category tags to require (`utility`, `worldgen`, …). */
  categories?: string[];
}

export interface AddonHit {
  project_id: string;
  slug: string;
  title: string;
  description: string;
  author: string;
  icon_url?: string | null;
  downloads: number;
  follows: number;
  /** Display categories, loader tags removed. */
  categories: string[];
  date_modified?: string | null;
  project_type: string;
  client_side?: string | null;
  server_side?: string | null;
  /** A jar of this project is already in the server's addon folder. */
  installed: boolean;
}

export interface AddonBrowseResult {
  hits: AddonHit[];
  total_hits: number;
  offset: number;
  limit: number;
}

export interface AddonFile {
  url: string;
  file_name: string;
  size: number;
  sha1?: string | null;
  sha512?: string | null;
}

export interface AddonDependencyRef {
  project_id?: string | null;
  version_id?: string | null;
  file_name?: string | null;
  dependency_type: "required" | "optional" | "incompatible" | "embedded" | string;
}

export interface AddonVersion {
  id: string;
  project_id: string;
  name: string;
  version_number: string;
  version_type: "release" | "beta" | "alpha" | string;
  date_published?: string | null;
  downloads: number;
  game_versions: string[];
  loaders: string[];
  /** Primary file — what an install downloads. */
  file: AddonFile;
  dependencies: AddonDependencyRef[];
  /** Runs on this server: exact game version and an accepted loader. */
  compatible: boolean;
}

export interface AddonProjectSummary {
  project_id: string;
  slug: string;
  title: string;
  description: string;
  icon_url?: string | null;
  project_type: string;
  client_side?: string | null;
  server_side?: string | null;
}

export interface ModrinthMatch {
  project: AddonProjectSummary;
  installed_version: AddonVersion;
  latest_version?: AddonVersion | null;
  update_available: boolean;
}

export interface InstalledAddonEntry {
  /** Exact file name on disk, including `.disabled` when disabled. Use it as the id. */
  file_name: string;
  /** File name without `.jar` / `.jar.disabled`; the fallback title. */
  display_name: string;
  enabled: boolean;
  size_bytes: number;
  modified?: string | null;
  sha1: string;
  /** Absent for jars Modrinth does not know (CurseForge, custom builds). */
  modrinth?: ModrinthMatch | null;
}

export interface InstalledAddonsReport {
  addons: InstalledAddonEntry[];
  /** `unavailable`: Modrinth unreachable — jars still listed, no metadata. */
  modrinth_status: "ok" | "unavailable";
  unknown_count: number;
  updates_available: number;
}

export interface AddonDependency {
  project_id: string;
  dependency_type: "required" | "optional" | "incompatible" | "embedded" | string;
  project?: AddonProjectSummary | null;
  installed: boolean;
  /** Best compatible version (resolved for required/optional deps only). */
  version?: AddonVersion | null;
}

export interface AddonInstallResult {
  file_name: string;
  project_id: string;
  version_id: string;
  version_number: string;
  /** The old jar removed because this one replaces it. */
  replaced?: string | null;
  /** The server was running: downloaded, queued until it stops. */
  pending?: boolean;
}

// -- addon changes waiting for the server to stop ----------------------------

export type PendingAddonAction =
  | {
      action: "install";
      file_name: string;
      staged: string;
      project_id?: string | null;
      version_number?: string | null;
      /** The jar on disk this one replaces (an update or version switch). */
      replace?: string | null;
      disabled: boolean;
    }
  | { action: "remove"; file_name: string }
  | { action: "set_enabled"; file_name: string; enabled: boolean };

export type PendingAddonChange = PendingAddonAction & {
  id: string;
  queued_at: string;
  title?: string | null;
  /** The last attempt to apply it failed; render like any backend error. */
  error?: { code: string; params?: Record<string, string>; detail?: string } | null;
};

// -- files: drag-and-drop import, disk usage --------------------------------

export type ConflictPolicy = "overwrite" | "keep_both" | "skip";

export interface ImportItem {
  name: string;
  isDir: boolean;
  exists: boolean;
  /**
   * What exists is the other kind (file vs folder). Overwrite cannot merge
   * those, so the backend keeps both instead.
   */
  typeMismatch?: boolean;
  files: number;
  bytes: number;
}

export interface ImportCheck {
  items: ImportItem[];
  /** Names (from `items`) that already exist in the target folder. */
  conflicts: string[];
  totalFiles: number;
  totalBytes: number;
}

export interface ImportProgress {
  importId: string;
  copiedFiles: number;
  totalFiles: number;
  copiedBytes: number;
  totalBytes: number;
  /** Instance-relative path being copied. */
  current?: string | null;
  done: boolean;
}

export interface ImportSummary {
  importId: string;
  copiedFiles: number;
  copiedBytes: number;
  imported: { sourceName: string; path: string; renamed: boolean }[];
  skipped: string[];
  /** `code` is an AppError code (translate like any error). */
  failed: { path: string; code: string }[];
}

export interface DiskUsage {
  totalBytes: number;
  fileCount: number;
  /** Biggest top-level entries, largest first (max 10). */
  largest: { name: string; isDir: boolean; bytes: number }[];
  computedAt: string;
}

/** A native (OS) drag over the window, positions in CSS pixels. */
export type WindowDragDrop =
  | { type: "enter"; paths: string[]; position: { x: number; y: number } }
  | { type: "over"; position: { x: number; y: number } }
  | { type: "drop"; paths: string[]; position: { x: number; y: number } }
  | { type: "leave" };

// -- console autocomplete ---------------------------------------------------

export interface CommandSuggestion {
  /** Without the leading slash (`gamemode`, `essentials:home`, `/wand`). */
  name: string;
  /** Argument syntax after the name (`<targets> [<reason>]`). */
  usage?: string | null;
  /** One-line description (Bukkit-style help), when the server gave one. */
  description?: string | null;
  source: "builtin" | "server";
  /** Zero-based argument index that takes a player name. */
  player_arg?: number | null;
}

export interface CompletionSet {
  /** Sorted by name, de-duplicated. */
  commands: CommandSuggestion[];
  /** Online players now. */
  players: string[];
  /** True when the running server contributed its own list. */
  from_server: boolean;
  fetched_at?: string | null;
}

/** Player name → UUID. Keys keep the game's spelling; compare case-insensitively. */
export type PlayerIdentities = Record<string, string>;

// -- Java runtimes -----------------------------------------------------------

export type JavaSource =
  | "nebula"
  | "settings"
  | "java_home"
  | "path"
  | "registry"
  | "system"
  | "minecraft_launcher"
  | "modrinth_app"
  | "curse_forge"
  | "prism_launcher"
  | "dev_tools";

export interface JavaInstallation {
  path: string;
  major: number;
  version: string;
  from_path: boolean;
  vendor?: string | null;
  /** `x86_64`, `aarch64`, `x86`… */
  arch?: string | null;
  source?: JavaSource | null;
}

export interface JavaRuntimes {
  /** False for games that do not run on a JVM. */
  applies: boolean;
  recommended_major?: number | null;
  /** The JVM this server is pinned to; absent = automatic. */
  selected?: string | null;
  /** What automatic resolves to right now. */
  automatic?: string | null;
  installations: JavaInstallation[];
  can_download: boolean;
}

export type JavaDownloadProgress =
  | { phase: "resolving" }
  | { phase: "downloading"; received: number; total?: number | null }
  | { phase: "extracting" }
  | { phase: "done"; path: string }
  | { phase: "failed"; code: string };

// -- log sharing (mclo.gs) ----------------------------------------------------

export type LogSource = { kind: "console" } | { kind: "latest" } | { kind: "crash_report"; name: string };

export interface LogFile {
  name: string;
  size_bytes: number;
  modified?: string | null;
}

export interface LogFiles {
  latest?: LogFile | null;
  crash_reports: LogFile[];
}

export interface SharedLog {
  id: string;
  url: string;
  raw?: string | null;
  /** Oldest lines left out to fit mclo.gs's limits. */
  omitted_lines: number;
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
  | { type: "players"; instanceId: string; online: string[] }
  | { type: "import"; instanceId: string; progress: ImportProgress }
  | { type: "javaDownload"; major: number; progress: JavaDownloadProgress }
  | { type: "pendingAddons"; instanceId: string; pending: PendingAddonChange[] }
  | { type: "transfer"; instanceId: string; progress: TransferProgress }
  | { type: "worldImport"; instanceId: string; progress: WorldImportProgress };

// -- transfers between machines ----------------------------------------------

export type TransferPhase = "prepare" | "export" | "upload" | "verify" | "adopt" | "finalize";

export interface TransferProgress {
  transferId: string;
  phase: TransferPhase;
  bytes: number;
  total: number;
  done: boolean;
  failed: boolean;
}

/** One end of a transfer, as the desktop that runs it reaches it. */
export type TransferEndpoint = { kind: "local" } | { kind: "remote"; address: string; token: string };

/** keep = leave the older copy (recommended); delete = remove its files. */
export type TransferMode = "keep" | "delete";

export interface TransferRequest {
  transferId?: string;
  instanceId: string;
  source: TransferEndpoint;
  target: TransferEndpoint;
  mode: TransferMode;
  allowPlayers?: boolean;
  keepBoth?: boolean;
}

export interface TransferExport {
  exportId: string;
  instanceId: string;
  name: string;
  sizeBytes: number;
  sha256: string;
  generation: number;
  sourceNodeId: string;
  sourceNodeName: string;
  exportedAt: string;
}

export interface TransferAdoptReport {
  instance: Instance;
  nodeId: string;
  nodeName: string;
  portChanged?: { from: number; to: number } | null;
  javaMissing?: number | null;
  replacedOlderCopy: boolean;
  runtimePathCleared: boolean;
  keptBoth: boolean;
}

export interface TransferOutcome {
  transferId: string;
  export: TransferExport;
  adopted: TransferAdoptReport;
  mode: TransferMode;
  commitWarning?: string | null;
}

// -- AI assistants (MCP) ------------------------------------------------------

export interface McpStatus {
  enabled: boolean;
  running: boolean;
  readOnly: boolean;
  port: number;
  url: string;
  token?: string | null;
  error?: string | null;
}

// -- remote access + UPnP -----------------------------------------------------

export interface RemoteStatus {
  enabled: boolean;
  running: boolean;
  port: number;
  addresses: string[];
  error?: string | null;
  /** The interface is served to browsers on the LAN. */
  webPanel: boolean;
}

export interface UpnpCheck {
  routerFound: boolean;
  routerAddress?: string | null;
  externalIp?: string | null;
  /** public | private (double NAT) | cgnat */
  externalKind?: string | null;
  mappingWorks: boolean;
  localIp?: string | null;
  failedStep?: string | null;
  detail?: string | null;
}

// -- Settings › Java and Diagnostics -----------------------------------------

export interface JavaSlot {
  major: number;
  configured?: string | null;
  effective?: JavaInstallation | null;
  automatic: boolean;
}

export interface JavaOverview {
  slots: JavaSlot[];
  installations: JavaInstallation[];
  canDownload: boolean;
}

export interface AppLog {
  path?: string | null;
  text: string;
  version: string;
  platform: string;
}

export interface WorldImportResult {
  levelName: string;
  dimensions: string[];
  sourceName: string;
  backupId?: string | null;
}

export interface WorldImportProgress {
  /** backup | unpack | place | done */
  phase: "backup" | "unpack" | "place" | "done";
  done: number;
  total: number;
}
