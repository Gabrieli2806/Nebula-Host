/**
 * Data access.
 *
 * Queries are cached by TanStack Query; live changes arrive as backend events
 * and are pushed straight into that cache, so a status change repaints without
 * a round trip and without a polling loop in the UI.
 */

import { useEffect, useMemo, useState } from "react";
import { useMutation, useQuery, useQueryClient, type QueryClient } from "@tanstack/react-query";

import { api, onAppEvent } from "@/lib/ipc";
import type { AppEvent, ConsoleLine, InstanceView } from "@/types/api";

export const keys = {
  node: ["node"] as const,
  nodeHealth: ["node", "health"] as const,
  requirements: ["node", "requirements"] as const,
  games: ["games"] as const,
  instances: ["instances"] as const,
  instance: (id: string) => ["instances", id] as const,
  console: (id: string) => ["console", id] as const,
  files: (id: string, path: string) => ["files", id, path] as const,
  backups: (id: string) => ["backups", id] as const,
  players: (id: string) => ["players", id] as const,
  playersOnline: (id: string) => ["players", id, "online"] as const,
  addons: (id: string) => ["addons", id] as const,
  settings: ["settings"] as const,
  instanceSettings: (id: string) => ["settings", id] as const,
};

/** Keep the console from growing without bound in a long session. */
const CONSOLE_LIMIT = 2000;

/** Wire backend events into the query cache. Mounted once, at the app root. */
export function useBackendEvents() {
  const client = useQueryClient();

  useEffect(() => onAppEvent((event) => applyEvent(client, event)), [client]);
}

function applyEvent(client: QueryClient, event: AppEvent) {
  switch (event.type) {
    case "status": {
      patchInstance(client, event.instanceId, (instance) => ({
        ...instance,
        status: event.status,
      }));
      break;
    }
    case "stats": {
      patchInstance(client, event.instanceId, (instance) => ({
        ...instance,
        ...event.state,
        // Status is owned by the status event; stats must never move it, or a
        // sleeping server flickers back to offline on every tick.
        status: instance.status,
      }));
      break;
    }
    case "players": {
      client.setQueryData(keys.playersOnline(event.instanceId), event.online);
      patchInstance(client, event.instanceId, (instance) => ({
        ...instance,
        players_online: event.online.length,
      }));
      break;
    }
    case "console": {
      client.setQueryData<ConsoleLine[]>(keys.console(event.instanceId), (lines = []) => {
        const next = [...lines, event.line];
        return next.length > CONSOLE_LIMIT ? next.slice(next.length - CONSOLE_LIMIT) : next;
      });
      break;
    }
    case "install":
      // Install progress is consumed by whoever is watching that install; see
      // `useInstallProgress`.
      break;
  }
}

function patchInstance(
  client: QueryClient,
  instanceId: string,
  patch: (instance: InstanceView) => InstanceView,
) {
  client.setQueryData<InstanceView[]>(keys.instances, (instances) =>
    instances?.map((instance) => (instance.id === instanceId ? patch(instance) : instance)),
  );
  client.setQueryData<InstanceView>(keys.instance(instanceId), (instance) =>
    instance ? patch(instance) : instance,
  );
}

/** Subscribe to install progress for one instance. */
export function useInstallProgress(instanceId: string | null) {
  // Keyed by instance, so switching to another install reads as "nothing yet"
  // without an effect that resets state on the way in.
  const [latest, setLatest] = useState<{
    instanceId: string;
    progress: Extract<AppEvent, { type: "install" }>["progress"];
  } | null>(null);

  useEffect(() => {
    if (!instanceId) return;
    return onAppEvent((event) => {
      if (event.type === "install" && event.instanceId === instanceId) {
        setLatest({ instanceId, progress: event.progress });
      }
    });
  }, [instanceId]);

  return latest && latest.instanceId === instanceId ? latest.progress : null;
}

// -- queries ----------------------------------------------------------------

export function useNode() {
  return useQuery({ queryKey: keys.node, queryFn: api.node.get });
}

export function useNodeHealth() {
  return useQuery({
    queryKey: keys.nodeHealth,
    queryFn: api.node.health,
    // The header shows live numbers; three seconds matches the backend tick.
    refetchInterval: 3000,
  });
}

export function useRequirements() {
  return useQuery({ queryKey: keys.requirements, queryFn: api.node.requirements });
}

export function useGames() {
  return useQuery({ queryKey: keys.games, queryFn: api.games.list, staleTime: Infinity });
}

export function useInstances() {
  return useQuery({ queryKey: keys.instances, queryFn: api.instances.list });
}

export function useInstance(instanceId: string) {
  const { data: instances } = useInstances();
  return useMemo(
    () => instances?.find((instance) => instance.id === instanceId) ?? null,
    [instances, instanceId],
  );
}

export function useConsole(instanceId: string) {
  return useQuery({
    queryKey: keys.console(instanceId),
    queryFn: () => api.instances.console(instanceId, 500),
    // Live lines arrive as events; refetching would fight with them.
    staleTime: Infinity,
    refetchOnWindowFocus: false,
  });
}

export function useAppSettings() {
  return useQuery({ queryKey: keys.settings, queryFn: api.settings.get });
}

// -- mutations --------------------------------------------------------------

/** Lifecycle actions share one hook so every caller gets the same refresh. */
export function useInstanceActions() {
  const client = useQueryClient();
  const invalidate = () => client.invalidateQueries({ queryKey: keys.instances });

  return {
    start: useMutation({ mutationFn: api.instances.start, onSettled: invalidate }),
    stop: useMutation({ mutationFn: api.instances.stop, onSettled: invalidate }),
    restart: useMutation({ mutationFn: api.instances.restart, onSettled: invalidate }),
    sleep: useMutation({ mutationFn: api.instances.sleep, onSettled: invalidate }),
    remove: useMutation({
      mutationFn: ({ id, deleteFiles }: { id: string; deleteFiles: boolean }) =>
        api.instances.remove(id, deleteFiles),
      onSettled: invalidate,
    }),
  };
}

export function useSaveSettings() {
  const client = useQueryClient();
  return useMutation({
    mutationFn: api.settings.set,
    onSuccess: (settings) => client.setQueryData(keys.settings, settings),
  });
}
