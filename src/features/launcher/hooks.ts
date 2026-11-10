/**
 * The launcher's shared queries, so every screen reads the same cache entry
 * under the same key (the server side does the same in hooks/useBackend.ts).
 * Launcher calls never go to a remote node: see launcherCall in lib/ipc.ts.
 */

import { useQuery } from "@tanstack/react-query";
import { launcherApi } from "@/lib/ipc";

export const launcherKeys = {
  profiles: ["launcher", "profiles"] as const,
  status: ["launcher", "status"] as const,
  capabilities: ["launcher", "capabilities"] as const,
  versions: ["launcher", "versions"] as const,
  accounts: ["launcher", "accounts"] as const,
  device: ["launcher", "device-graphics"] as const,
  content: (id: string | undefined) => ["launcher", "content", id] as const,
};

export const useLauncherProfiles = () => useQuery({ queryKey: launcherKeys.profiles, queryFn: launcherApi.profiles });
export const useLauncherCapabilities = () => useQuery({ queryKey: launcherKeys.capabilities, queryFn: launcherApi.capabilities, staleTime: Infinity });
/** Every instance's phase, polled while the launcher is open. */
export const useLauncherStatus = () => useQuery({ queryKey: launcherKeys.status, queryFn: launcherApi.status, refetchInterval: 1000 });
export const useGameVersions = () => useQuery({ queryKey: launcherKeys.versions, queryFn: launcherApi.versions, staleTime: 300_000 });
/** The phone's GPU (Vulkan, OpenGL ES, renderers in this build); null on the desktop. */
export const useDeviceGraphics = (enabled = true) => useQuery({ queryKey: launcherKeys.device, queryFn: launcherApi.deviceGraphics, staleTime: Infinity, enabled });

/** What a phase means for the status dot and its label. */
export function phaseDot(phase?: string): "online" | "starting" | "crashed" | "offline" {
  return phase === "running" ? "online" : phase === "installing" || phase === "stopping" ? "starting" : phase === "failed" ? "crashed" : "offline";
}
export function phaseLabel(phase?: string): string {
  return phase === "running" ? "launcher.runningShort" : phase === "installing" ? "launcher.installingShort" : phase === "failed" ? "launcher.failedShort" : "status.offline";
}
export const isBusy = (phase?: string) => phase === "running" || phase === "installing" || phase === "stopping";
