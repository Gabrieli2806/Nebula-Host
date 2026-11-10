/**
 * What this device's build allows. The regular build allows everything; the
 * Google Play build installs no mod loaders, mods, modpacks or shaders, and
 * hosts no servers on the phone (src-tauri/src/flavor.rs).
 *
 * The backend enforces those rules; this only lets the UI leave out what it
 * would refuse. It describes *this* device: managing another machine is
 * decided by that machine's own build. Until the answer arrives (and in the
 * browser panel) everything is offered, and the backend still has the last
 * word.
 */

import { useQuery } from "@tanstack/react-query";

import type { ContentKind, Loader } from "@/features/launcher/types";
import { launcherApi } from "@/lib/ipc";
import { IS_WEB } from "@/lib/web";

const ALL_LOADERS: Loader[] = ["vanilla", "fabric", "forge", "neoforge", "quilt"];
const ALL_KINDS: ContentKind[] = ["mod", "modpack", "resourcepack", "shader"];

export function useDeviceBuild() {
  const caps = useQuery({
    queryKey: ["launcher", "capabilities"],
    queryFn: launcherApi.capabilities,
    enabled: !IS_WEB,
    staleTime: Infinity,
  });
  const playStore = caps.data?.flavor === "play_store";
  return {
    /** The Google Play build. */
    playStore,
    /** Whether this device may host servers itself. */
    hosting: !playStore,
    loaders: caps.data?.loaders ?? ALL_LOADERS,
    contentKinds: caps.data?.content_kinds ?? ALL_KINDS,
  };
}
