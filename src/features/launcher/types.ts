export type Loader = "vanilla" | "fabric" | "forge" | "neoforge" | "quilt";
export type ContentKind = "mod" | "modpack" | "resourcepack" | "shader";
export interface LauncherProfile {
  id: string; name: string; game_version: string; loader: Loader; memory_mb: number; loader_version?: string | null; icon?: string | null;
}
export type CreateProfile = Omit<LauncherProfile, "id" | "loader_version" | "icon">;
export interface LauncherCapabilities { platform: string; can_launch: boolean }
export interface ClientStatus { phase: string; stage: string; completed: number; total: number; error: import("@/lib/ipc").AppError | null; exit_code: number | null }
export interface GameVersion { id: string; type: string; client_supported: boolean }
export interface LauncherAccount { id: string; name: string }
export interface SkinInfo { url: string | null; variant: "classic" | "slim"; capes: { id: string; alias: string; url: string; active: boolean }[] }
export interface DeviceLogin { id: string; user_code: string; verification_uri: string; expires_in: number; interval: number }
export type LoginResult = { status: "pending"; interval: number } | { status: "complete"; account: LauncherAccount };
export interface LauncherProject { project_id: string; slug: string; title: string; description: string; author: string; downloads: number; project_type: string; icon_url: string | null; categories?: string[]; date_modified?: string | null; follows?: number }
export interface LauncherSearch { hits: LauncherProject[]; total_hits: number }
export interface ContentStatus {
  kind: Exclude<ContentKind, "modpack">; file: string; project_id: string | null; title: string | null; icon_url: string | null;
  version: string | null; compatible: boolean; update: { version_id: string; version_number: string; file_name: string } | null;
}
export interface InstalledContent { kind: Exclude<ContentKind, "modpack">; file: string; size: number; enabled: boolean; project_id: string | null }
