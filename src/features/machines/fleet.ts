/**
 * Every machine this window knows, asked all at once: this PC (on the
 * desktop) and every paired node, each with its health and its servers.
 * One query per machine, so a machine that does not answer only blanks its
 * own card.
 */

import { useQueries } from "@tanstack/react-query";

import { activeRemote, remoteNodes, switchTo, type RemoteNode } from "@/lib/connection";
import { callMachine } from "@/lib/ipc";
import { IS_WEB } from "@/lib/web";
import type { InstanceView, Node, NodeHealth } from "@/types/api";

export interface FleetMachine {
  /** "local" or the connection key of a paired node. */
  key: string;
  remote: RemoteNode | null;
  /** Where it answers, for the subtitle ("This PC" for local). */
  address: string | null;
  /** This window is managing it right now. */
  managed: boolean;
}

export interface FleetEntry extends FleetMachine {
  name: string;
  node?: Node;
  health?: NodeHealth;
  instances?: InstanceView[];
  loading: boolean;
  reachable: boolean;
}

function machines(): FleetMachine[] {
  const active = activeRemote();
  // The web panel only knows the machine that served it.
  if (IS_WEB) return active ? [{ key: active.id, remote: active, address: active.address, managed: true }] : [];
  return [
    { key: "local", remote: null, address: null, managed: active === null },
    ...remoteNodes().map((node) => ({ key: node.id, remote: node, address: node.address, managed: active?.id === node.id })),
  ];
}

interface Snapshot {
  node: Node;
  health: NodeHealth;
  instances: InstanceView[];
}

export function useFleet(): FleetEntry[] {
  const list = machines();
  const results = useQueries({
    queries: list.map((machine) => ({
      queryKey: ["fleet", machine.key],
      queryFn: async (): Promise<Snapshot> => {
        const [node, health, instances] = await Promise.all([
          callMachine<Node>(machine.remote, "node_get"),
          callMachine<NodeHealth>(machine.remote, "node_health"),
          callMachine<InstanceView[]>(machine.remote, "instances_list"),
        ]);
        return { node, health, instances };
      },
      refetchInterval: 5_000,
      retry: false,
    })),
  });
  return list.map((machine, index) => {
    const result = results[index];
    const data = result?.data;
    return {
      ...machine,
      name: data?.node.name ?? machine.remote?.name ?? "…",
      node: data?.node,
      health: data?.health,
      instances: data?.instances,
      loading: result?.isLoading ?? true,
      reachable: !result?.isError,
    };
  });
}

/** Open a server on whichever machine has it. */
export function openOnMachine(machine: FleetMachine, instanceId: string) {
  window.location.hash = `#/servers/${instanceId}/overview`;
  if (!machine.managed) switchTo(machine.key);
}

/** Manage a machine (its server list). */
export function manageMachine(machine: FleetMachine) {
  window.location.hash = "#/servers";
  if (!machine.managed) switchTo(machine.key);
}
