/**
 * Data access.
 *
 * Queries are cached by TanStack Query; live changes arrive as backend events
 * and are pushed straight into that cache, so a status change repaints without
 * a round trip and without a polling loop in the UI.
 */

import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  keepPreviousData,
  useMutation,
  useQuery,
  useQueryClient,
  type QueryClient,
} from "@tanstack/react-query";

import { agreementRequired, requestAgreement } from "@/lib/agreement";
import { api, onAppEvent, onWindowDragDrop, toAppError, type AppError } from "@/lib/ipc";
import type {
  AddonBrowseRequest,
  AppEvent,
  CompletionSet,
  ConflictPolicy,
  ConsoleLine,
  Distribution,
  ImportProgress,
  ImportSummary,
  InstanceView,
  JavaDownloadProgress,
  PendingAddonChange,
  PlayerIdentities,
  WindowDragDrop,
} from "@/types/api";

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
  addonContext: (id: string) => ["addons", id, "context"] as const,
  addonsList: (id: string) => ["addons", id, "list"] as const,
  addonBrowse: (id: string, request: AddonBrowseRequest) => ["addons", id, "browse", request] as const,
  addonVersions: (id: string, projectId: string, all: boolean) =>
    ["addons", id, "versions", projectId, all] as const,
  addonDependencies: (id: string, versionId: string) => ["addons", id, "deps", versionId] as const,
  addonsPending: (id: string) => ["addons", id, "pending"] as const,
  javaRuntimes: (id: string) => ["java", id] as const,
  logFiles: (id: string) => ["logs", id] as const,
  completions: (id: string) => ["completions", id] as const,
  // Not under ["instances"]: every start/stop/settings save invalidates that
  // prefix, and the folder size does not change with any of them.
  diskUsage: (id: string) => ["disk", id] as const,
  playerIdentities: (id: string) => ["players", id, "identities"] as const,
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
      // The server's own command list is per run: online brings it, a stop
      // or restart makes it stale.
      void client.invalidateQueries({ queryKey: keys.completions(event.instanceId) });
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
      client.setQueryData<CompletionSet>(keys.completions(event.instanceId), (set) =>
        set ? { ...set, players: event.online } : set,
      );
      // A joiner's UUID is announced on join; pick it up for their face.
      void client.invalidateQueries({ queryKey: keys.playerIdentities(event.instanceId) });
      patchInstance(client, event.instanceId, (instance) => ({
        ...instance,
        players_online: event.online.length,
      }));
      break;
    }
    case "console": {
      // Only append to a console that has loaded its history. Creating the
      // query here would leave it holding one line, and with staleTime
      // Infinity the history fetch would never run. Returning undefined
      // leaves the cache untouched; the first fetch brings the line with it.
      client.setQueryData<ConsoleLine[]>(keys.console(event.instanceId), (lines) => {
        if (!lines) return lines;
        const next = [...lines, event.line];
        return next.length > CONSOLE_LIMIT ? next.slice(next.length - CONSOLE_LIMIT) : next;
      });
      break;
    }
    case "install":
      // Install progress is consumed by whoever is watching that install; see
      // `useInstallProgress`.
      break;
    case "import":
      // Consumed by `useImportFiles`.
      break;
    case "pendingAddons": {
      // The queue changed (queued, cancelled, applied): the event carries it
      // whole. When something applied, the folder changed too.
      const before = client.getQueryData<PendingAddonChange[]>(keys.addonsPending(event.instanceId));
      client.setQueryData(keys.addonsPending(event.instanceId), event.pending);
      if (before && event.pending.length < before.length) {
        void client.invalidateQueries({ queryKey: keys.addonsList(event.instanceId) });
        void client.invalidateQueries({ queryKey: ["addons", event.instanceId, "browse"] });
      }
      break;
    }
    case "javaDownload":
      // Consumed by `useJavaDownload`.
      break;
    case "transfer":
      // Consumed by the transfer dialog. When one ends, the server may have
      // left (or arrived), so the list is re-read.
      if (event.progress.done) void client.invalidateQueries({ queryKey: keys.instances });
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

/** The manifest entry a server was built from — its name, and its colour. */
export function useDistribution(instance: InstanceView | null): Distribution | null {
  const { data: games } = useGames();
  return useMemo(() => {
    if (!instance) return null;
    const game = games?.find((candidate) => candidate.id === instance.game);
    return (
      game?.distributions.find(
        (candidate) => candidate.id === instance.build.distribution,
      ) ?? null
    );
  }, [games, instance]);
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

/**
 * Start or sleep, unless the server still needs the game's agreement (the
 * Minecraft EULA): then the agreement dialog asks for it (AgreementGate) and
 * repeats the action once the user accepts. That is not a failure to show.
 */
function gated(resume: "start" | "sleep", run: (instanceId: string) => Promise<void>) {
  return async (instanceId: string) => {
    try {
      await run(instanceId);
    } catch (error) {
      const needed = agreementRequired(error);
      if (!needed) throw error;
      requestAgreement({ instanceId, url: needed.url, resume });
    }
  };
}

/** Lifecycle actions share one hook so every caller gets the same refresh. */
export function useInstanceActions() {
  const client = useQueryClient();
  const invalidate = () => client.invalidateQueries({ queryKey: keys.instances });

  return {
    start: useMutation({ mutationFn: gated("start", api.instances.start), onSettled: invalidate }),
    stop: useMutation({ mutationFn: api.instances.stop, onSettled: invalidate }),
    restart: useMutation({ mutationFn: api.instances.restart, onSettled: invalidate }),
    sleep: useMutation({ mutationFn: gated("sleep", api.instances.sleep), onSettled: invalidate }),
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

// -- round 3: resources, completion, players, addons, import ---------------

/** Disk used by the instance folder (backend caches 60 s). */
export function useDiskUsage(instanceId: string) {
  return useQuery({
    queryKey: keys.diskUsage(instanceId),
    queryFn: () => api.instances.diskUsage(instanceId),
    staleTime: 60_000,
    refetchInterval: 60_000,
  });
}

/**
 * Console autocomplete set. The first fetch while online may take a few
 * seconds (the backend runs a hidden `help`); it is refetched automatically
 * when the server's status changes and its `players` follow join/leave
 * events. Builtin commands are returned even when the server is offline.
 */
export function useConsoleCompletions(instanceId: string, enabled = true) {
  return useQuery({
    queryKey: keys.completions(instanceId),
    queryFn: () => api.instances.completions(instanceId),
    enabled,
    staleTime: Infinity,
    refetchOnWindowFocus: false,
  });
}

/** Name → UUID for faces. */
export function usePlayerIdentities(instanceId: string) {
  return useQuery({
    queryKey: keys.playerIdentities(instanceId),
    queryFn: () => api.players.identities(instanceId),
    staleTime: 5 * 60_000,
  });
}

/** Case-insensitive UUID lookup in a `PlayerIdentities` map. */
export function findUuid(identities: PlayerIdentities | undefined, name: string): string | undefined {
  if (!identities) return undefined;
  if (identities[name]) return identities[name];
  const lower = name.toLowerCase();
  for (const key in identities) if (key.toLowerCase() === lower) return identities[key];
  return undefined;
}

export function useAddonContext(instanceId: string) {
  return useQuery({
    queryKey: keys.addonContext(instanceId),
    queryFn: () => api.addons.context(instanceId),
    staleTime: Infinity,
  });
}

/** Installed jars, hash-identified. Hashing is cached backend-side. */
export function useInstalledAddons(instanceId: string, enabled = true) {
  return useQuery({
    queryKey: keys.addonsList(instanceId),
    queryFn: () => api.addons.list(instanceId),
    enabled,
    staleTime: 60_000,
  });
}

/** One page of Modrinth results; keeps the previous page while paging. */
export function useAddonBrowse(instanceId: string, request: AddonBrowseRequest, enabled = true) {
  return useQuery({
    queryKey: keys.addonBrowse(instanceId, request),
    queryFn: () => api.addons.browse(instanceId, request),
    enabled,
    staleTime: 5 * 60_000,
    placeholderData: keepPreviousData,
  });
}

export function useAddonVersions(instanceId: string, projectId: string | null, all = false) {
  return useQuery({
    queryKey: keys.addonVersions(instanceId, projectId ?? "", all),
    queryFn: () => api.addons.versions(instanceId, projectId as string, all),
    enabled: !!projectId,
    staleTime: 5 * 60_000,
  });
}

export function useAddonDependencies(instanceId: string, versionId: string | null) {
  return useQuery({
    queryKey: keys.addonDependencies(instanceId, versionId ?? ""),
    queryFn: () => api.addons.dependencies(instanceId, versionId as string),
    enabled: !!versionId,
    staleTime: 5 * 60_000,
  });
}

/** Addon changes waiting for the server to stop. Kept live by events. */
export function usePendingAddons(instanceId: string, enabled = true) {
  return useQuery({
    queryKey: keys.addonsPending(instanceId),
    queryFn: () => api.addons.pending(instanceId),
    enabled,
    staleTime: Infinity,
  });
}

export function usePendingAddonMutations(instanceId: string) {
  const client = useQueryClient();
  const store = (queue: PendingAddonChange[]) => client.setQueryData(keys.addonsPending(instanceId), queue);
  return {
    cancel: useMutation({
      mutationFn: (changeId: string) => api.addons.cancelPending(instanceId, changeId),
      onSuccess: store,
    }),
    apply: useMutation({
      mutationFn: () => api.addons.applyPending(instanceId),
      onSuccess: (queue) => {
        store(queue);
        return client.invalidateQueries({ queryKey: keys.addons(instanceId) });
      },
    }),
  };
}

/** Every JVM on the node, and which one this server's version wants. */
export function useJavaRuntimes(instanceId: string) {
  return useQuery({
    queryKey: keys.javaRuntimes(instanceId),
    queryFn: () => api.java.runtimes(instanceId),
    staleTime: 60_000,
  });
}

/**
 * Download a Temurin runtime with live progress. Downloads are per major,
 * not per server, so progress is keyed by major too.
 */
export function useJavaDownload() {
  const client = useQueryClient();
  const [progress, setProgress] = useState<{ major: number; progress: JavaDownloadProgress } | null>(null);

  useEffect(
    () =>
      onAppEvent((event) => {
        if (event.type === "javaDownload") setProgress({ major: event.major, progress: event.progress });
      }),
    [],
  );

  const download = useMutation({
    mutationFn: (major: number) => api.java.download(major),
    onSettled: () => client.invalidateQueries({ queryKey: ["java"] }),
  });

  return { download, progress };
}

/** Every addon write, each refreshing the installed list and search badges. */
export function useAddonMutations(instanceId: string) {
  const client = useQueryClient();
  // `keys.addons` is a prefix of the pending queue's key too, so a write
  // that got queued (server running) refreshes the pending list with it.
  const invalidate = () =>
    Promise.all([
      client.invalidateQueries({ queryKey: keys.addons(instanceId) }),
      client.invalidateQueries({ queryKey: keys.completions(instanceId) }),
    ]);

  return {
    installVersion: useMutation({
      mutationFn: (args: { projectId: string; versionId?: string; replace?: string }) =>
        api.addons.installVersion(instanceId, args.projectId, args.versionId, args.replace),
      onSettled: invalidate,
    }),
    update: useMutation({
      mutationFn: (fileName: string) => api.addons.update(instanceId, fileName),
      onSettled: invalidate,
    }),
    setEnabled: useMutation({
      mutationFn: (args: { fileName: string; enabled: boolean }) =>
        api.addons.setEnabled(instanceId, args.fileName, args.enabled),
      onSettled: invalidate,
    }),
    remove: useMutation({
      mutationFn: (fileName: string) => api.addons.remove(instanceId, fileName),
      onSettled: invalidate,
    }),
  };
}

/**
 * Native drag-and-drop over the window. `handler` always sees the latest
 * closure; pass `enabled = false` to stop listening (e.g. while a dialog is
 * open).
 */
export function useWindowDragDrop(handler: (event: WindowDragDrop) => void, enabled = true) {
  const latest = useRef(handler);
  useEffect(() => {
    latest.current = handler;
  });
  useEffect(() => {
    if (!enabled) return;
    return onWindowDragDrop((event) => latest.current(event));
  }, [enabled]);
}

/**
 * Import dropped paths into an instance folder, with live progress.
 * `run` resolves with the summary (or rejects with an AppError); `progress`
 * tracks the import currently running from this hook.
 */
export function useImportFiles(instanceId: string) {
  const client = useQueryClient();
  const [progress, setProgress] = useState<ImportProgress | null>(null);
  const [summary, setSummary] = useState<ImportSummary | null>(null);
  const [error, setError] = useState<AppError | null>(null);
  const [running, setRunning] = useState(false);
  const current = useRef<string | null>(null);

  useEffect(
    () =>
      onAppEvent((event) => {
        if (event.type === "import" && event.instanceId === instanceId && event.progress.importId === current.current) {
          setProgress(event.progress);
        }
      }),
    [instanceId],
  );

  const run = useCallback(
    async (target: string, sources: string[], conflict: ConflictPolicy = "keep_both") => {
      const importId =
        typeof crypto !== "undefined" && "randomUUID" in crypto
          ? crypto.randomUUID()
          : `import-${Date.now()}-${Math.random().toString(16).slice(2)}`;
      current.current = importId;
      setRunning(true);
      setError(null);
      setSummary(null);
      setProgress(null);
      try {
        const result = await api.files.import(instanceId, target, sources, conflict, importId);
        setSummary(result);
        return result;
      } catch (raw) {
        const appError = toAppError(raw);
        setError(appError);
        throw appError;
      } finally {
        setRunning(false);
        void client.invalidateQueries({ queryKey: ["files", instanceId] });
        void client.invalidateQueries({ queryKey: keys.diskUsage(instanceId) });
        void client.invalidateQueries({ queryKey: keys.addons(instanceId) });
      }
    },
    [client, instanceId],
  );

  const check = useCallback(
    (target: string, sources: string[]) => api.files.importCheck(instanceId, target, sources),
    [instanceId],
  );

  const reset = useCallback(() => {
    current.current = null;
    setProgress(null);
    setSummary(null);
    setError(null);
  }, []);

  return { run, check, reset, progress, summary, error, running };
}
