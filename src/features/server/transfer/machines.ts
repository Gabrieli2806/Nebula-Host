/**
 * The machines a server can move between: this PC and every node paired with
 * it. Each carries its node id (to find "where did it go"), and the endpoint
 * the desktop backend uses to reach it.
 */

import { useQuery } from "@tanstack/react-query";

import { activeRemote, endpointFor, resolveNodeIds } from "@/lib/connection";
import { api } from "@/lib/ipc";
import type { InstanceView, TransferEndpoint } from "@/types/api";

export interface Machine {
  /** "local", or the connection key of a paired node. */
  key: string;
  name: string;
  /** The node's own id; unknown for a node that has not answered yet. */
  nodeId?: string;
  endpoint: TransferEndpoint;
  isThisPc: boolean;
}

/** The connection key of the machine this window manages. */
export function managedKey(): string {
  return activeRemote()?.id ?? "local";
}

export function useMachines(thisPcName: string) {
  return useQuery({
    queryKey: ["transfer", "machines"],
    queryFn: async (): Promise<Machine[]> => {
      const [local, remotes] = await Promise.all([api.node.getLocal().catch(() => null), resolveNodeIds()]);
      return [
        { key: "local", name: local?.name ?? thisPcName, nodeId: local?.id, endpoint: { kind: "local" }, isThisPc: true },
        ...remotes.map((node) => ({
          key: node.id,
          name: node.name,
          nodeId: node.nodeId,
          endpoint: endpointFor(node),
          isThisPc: false,
        })),
      ];
    },
    staleTime: 30_000,
  });
}

/** Where a server lives now, found by the node id a transfer recorded. */
export function machineByNodeId(machines: Machine[] | undefined, nodeId: string): Machine | undefined {
  return machines?.find((machine) => machine.nodeId === nodeId);
}

/** The machine an older copy's server moved to, when this copy is one. */
export function movedTo(instance: InstanceView): string | null {
  return instance.transfer?.state === "transferred_out" ? instance.transfer.to_node_name : null;
}
