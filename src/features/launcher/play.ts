/**
 * One instance's game state and its keys (play, close, return to the game),
 * shared by the instance page, the library cards and the sidebar, so all of
 * them offer the same thing at the same time.
 */

import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";

import { launcherApi } from "@/lib/ipc";
import { usePrimaryAccount } from "./primaryAccount";
import type { LauncherProfile } from "./types";

export function phaseDot(phase?: string): "online" | "starting" | "crashed" | "offline" {
  return phase === "running" ? "online" : phase === "installing" || phase === "stopping" ? "starting" : phase === "failed" ? "crashed" : "offline";
}

export function phaseLabel(phase?: string): string {
  return phase === "running" ? "launcher.runningShort" : phase === "installing" ? "launcher.installingShort" : phase === "failed" ? "launcher.failedShort" : "status.offline";
}

export function useLauncherStatus() {
  return useQuery({ queryKey: ["launcher", "status"], queryFn: launcherApi.status, refetchInterval: 1000 });
}

export function useLauncherCapabilities() {
  return useQuery({ queryKey: ["launcher", "capabilities"], queryFn: launcherApi.capabilities });
}

export function usePlayState(profile: LauncherProfile) {
  const client = useQueryClient();
  const { primary } = usePrimaryAccount();
  const states = useLauncherStatus();
  const caps = useLauncherCapabilities();
  const state = states.data?.[profile.id];
  const refresh = () => client.invalidateQueries({ queryKey: ["launcher", "status"] });
  const start = useMutation({ mutationFn: () => launcherApi.start(profile.id, primary ? "account" : "demo", primary?.id), onSuccess: refresh });
  const stop = useMutation({ mutationFn: () => launcherApi.stop(profile.id), onSuccess: refresh });
  const resume = useMutation({ mutationFn: () => launcherApi.resume(profile.id) });

  // Only the most recently pressed key speaks, as on a server's controls.
  const latest = [start, stop, resume].reduce((a, b) => (b.submittedAt > a.submittedAt ? b : a));

  return {
    state,
    phase: state?.phase,
    installing: state?.phase === "installing",
    running: state?.phase === "running" || state?.phase === "stopping",
    /** Android runs the game in its own activity: offer the way back to it. */
    canReturn: state?.phase === "running" && caps.data?.platform === "android",
    /** One download at a time: another instance installing blocks Play. */
    anyInstalling: Object.values(states.data ?? {}).some((s) => s.phase === "installing"),
    canLaunch: !!caps.data?.can_launch && profile.loader !== "quilt" && (caps.data.loaders?.includes(profile.loader) ?? true),
    /** The Google Play build does not run mod loaders (src-tauri/src/flavor.rs). */
    loaderNotInBuild: caps.data?.loaders ? !caps.data.loaders.includes(profile.loader) && profile.loader !== "quilt" : false,
    platform: caps.data?.platform,
    withAccount: !!primary,
    start,
    stop,
    resume,
    error: latest.error,
  };
}
