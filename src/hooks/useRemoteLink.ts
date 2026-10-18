import { useSyncExternalStore } from "react";

import { activeRemote, linkState, subscribeLink, type RemoteNode } from "@/lib/connection";

/**
 * The machine this window manages and whether it is answering. `remote` is
 * null when managing this PC (which is always reachable).
 */
export function useRemoteLink(): { remote: RemoteNode | null; reachable: boolean } {
  const state = useSyncExternalStore(subscribeLink, linkState, linkState);
  const remote = activeRemote();
  return { remote, reachable: remote === null || state === "up" };
}
