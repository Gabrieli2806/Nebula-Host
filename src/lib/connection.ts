/**
 * Which machine this window is managing: this one (local, over Tauri IPC) or a
 * paired node elsewhere on the LAN (over its HTTP API).
 *
 * The choice and the paired nodes live in localStorage — they belong to this
 * desktop, not to any node. Switching reloads the window so every query and
 * event listener starts clean against the new machine.
 */

export interface RemoteNode {
  /** Stable key for this connection: `remote:<host:port>`. */
  id: string;
  /** The node's own id (unique per machine since transfers). Filled at
   *  pairing, or later by `resolveNodeIds` for nodes paired before. */
  nodeId?: string;
  name: string;
  /** host:port */
  address: string;
  token: string;
  deviceId: string;
  platform?: string;
}

const NODES_KEY = "nebula.remoteNodes";
const ACTIVE_KEY = "nebula.activeNode";

function read<T>(key: string, fallback: T): T {
  try {
    const raw = window.localStorage.getItem(key);
    return raw ? (JSON.parse(raw) as T) : fallback;
  } catch {
    return fallback;
  }
}

function write(key: string, value: unknown) {
  try {
    window.localStorage.setItem(key, JSON.stringify(value));
  } catch {
    // Storage blocked: nothing persists, the session still works.
  }
}

/**
 * Whether the machine being managed is answering. Set by every RPC and by the
 * event stream: a network failure means "down", any answer (even an error)
 * means "up". Only meaningful while a remote node is active.
 */
export type LinkState = "up" | "down";

let link: LinkState = "up";
const linkListeners = new Set<() => void>();

export function linkState(): LinkState {
  return link;
}

export function subscribeLink(listener: () => void): () => void {
  linkListeners.add(listener);
  return () => {
    linkListeners.delete(listener);
  };
}

/** Only answers from the machine this window manages move the link: a call
 *  to another node (the transfer dialog asks every paired one), or a slow
 *  one that outlived a switch, says nothing about it. */
function reportLink(nodeId: string, next: LinkState) {
  if (activeRemote()?.id !== nodeId) return;
  if (link === next) return;
  link = next;
  for (const listener of linkListeners) listener();
}

function isUnreachable(error: unknown): boolean {
  return typeof error === "object" && error !== null && (error as { code?: unknown }).code === "node_unreachable";
}

export function remoteNodes(): RemoteNode[] {
  return read<RemoteNode[]>(NODES_KEY, []);
}

/** The remote node this window manages, or null when it manages this PC. */
export function activeRemote(): RemoteNode | null {
  const id = read<string>(ACTIVE_KEY, "local");
  if (id === "local") return null;
  return remoteNodes().find((node) => node.id === id) ?? null;
}

export function isRemote(): boolean {
  return activeRemote() !== null;
}

export function switchTo(id: string | "local") {
  write(ACTIVE_KEY, id);
  window.location.reload();
}

export function forgetNode(id: string) {
  write(
    NODES_KEY,
    remoteNodes().filter((node) => node.id !== id),
  );
  if (read<string>(ACTIVE_KEY, "local") === id) switchTo("local");
}

function normalise(address: string): string {
  const trimmed = address.trim().replace(/^https?:\/\//, "").replace(/\/+$/, "");
  return /:\d+$/.test(trimmed) ? trimmed : `${trimmed}:47400`;
}

export interface NodeInfo {
  app: string;
  version: string;
  api: number;
  nodeId: string;
  nodeName: string;
  platform: string;
}

/** A typed failure, shaped like the backend's AppError. */
function failure(code: string, detail?: string, params: Record<string, string> = {}) {
  return { code, params, detail };
}

async function request<T>(url: string, init: RequestInit, timeoutMs = 15000): Promise<T> {
  const controller = new AbortController();
  const timer = window.setTimeout(() => controller.abort(), timeoutMs);
  let response: Response;
  try {
    response = await fetch(url, { ...init, signal: controller.signal });
  } catch (error) {
    throw failure("node_unreachable", String(error), { address: url.split("/")[2] ?? "" });
  } finally {
    window.clearTimeout(timer);
  }
  const text = await response.text();
  const body = text ? JSON.parse(text) : null;
  if (!response.ok) {
    if (body && typeof body.code === "string") {
      // Give remote-specific reasons their own codes, so the UI can say
      // exactly what went wrong instead of a generic "invalid input".
      const reason = body.params?.reason as string | undefined;
      const specific: Record<string, string> = {
        not_paired: "not_paired",
        code_mismatch: "pairing_code_wrong",
        code_expired: "pairing_code_expired",
        no_pending_code: "pairing_code_expired",
        local_only: "local_only",
      };
      if (reason && specific[reason]) throw { ...body, code: specific[reason] };
      throw body;
    }
    throw failure("unexpected", text || response.statusText);
  }
  return body as T;
}

export async function probe(address: string): Promise<NodeInfo> {
  const info = await request<NodeInfo>(`http://${normalise(address)}/info`, { method: "GET" }, 6000);
  if (info.app !== "nebula-host") throw failure("not_a_node");
  return info;
}

/** Trade the code shown on the node for a token, remember the node. */
export async function pair(address: string, code: string, deviceName: string): Promise<RemoteNode> {
  const host = normalise(address);
  const info = await probe(host);
  const result = await request<{ token: string; deviceId: string; node: { id: string; name: string } }>(
    `http://${host}/pair`,
    {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ code: code.replace(/\D/g, ""), deviceName }),
    },
  );
  const node: RemoteNode = {
    id: `remote:${host}`,
    name: result.node.name,
    address: host,
    token: result.token,
    deviceId: result.deviceId,
    platform: info.platform,
    nodeId: info.nodeId,
  };
  write(NODES_KEY, [...remoteNodes().filter((n) => n.id !== node.id), node]);
  return node;
}

/**
 * Ask every paired node who it is and remember its node id. A node migrated
 * from the old shared id ("local") reports its new one here. Unreachable
 * nodes keep what was known. Returns the refreshed list.
 */
export async function resolveNodeIds(): Promise<RemoteNode[]> {
  const nodes = remoteNodes();
  const answers = await Promise.all(
    nodes.map((node) =>
      request<NodeInfo>(`http://${node.address}/info`, { method: "GET" }, 5000).catch(() => null),
    ),
  );
  let changed = false;
  const next = nodes.map((node, index) => {
    const info = answers[index];
    if (!info || info.app !== "nebula-host" || info.nodeId === node.nodeId) return node;
    changed = true;
    return { ...node, nodeId: info.nodeId };
  });
  if (changed) write(NODES_KEY, next);
  return next;
}

/** How the desktop backend reaches a machine for a transfer. */
export function endpointFor(node: RemoteNode | null): { kind: "local" } | { kind: "remote"; address: string; token: string } {
  return node ? { kind: "remote", address: node.address, token: node.token } : { kind: "local" };
}

/** Calls the sidebar makes every few seconds: they must fail fast, so an
 *  unplugged machine shows as unreachable in seconds, not minutes. */
const QUICK_COMMANDS = new Set(["node_health", "node_get"]);
/** Read-only lists and lookups the pages open with: a node that does not
 *  answer them in this long is not going to, and the page should say so
 *  instead of loading for minutes. */
const READ_COMMANDS = new Set(["instances_list", "instance_get", "games_list", "players_online", "backups_list"]);

function timeoutFor(command: string): number {
  if (QUICK_COMMANDS.has(command)) return 8000;
  if (READ_COMMANDS.has(command)) return 20000;
  return 120000;
}

export async function remoteCall<T>(node: RemoteNode, command: string, args?: Record<string, unknown>): Promise<T> {
  try {
    const result = await request<T>(
      `http://${node.address}/rpc`,
      {
        method: "POST",
        headers: { "Content-Type": "application/json", Authorization: `Bearer ${node.token}` },
        body: JSON.stringify({ command, args: args ?? {} }),
      },
      timeoutFor(command),
    );
    reportLink(node.id, "up");
    return result;
  } catch (error) {
    reportLink(node.id, isUnreachable(error) ? "down" : "up");
    throw error;
  }
}

/**
 * Subscribe to a node's event stream (server-sent events over fetch, so the
 * token can travel in a header). Reconnects with backoff. Returns a stop
 * function.
 */
export function remoteEvents(node: RemoteNode, handler: (payload: unknown) => void): () => void {
  let stopped = false;
  let controller: AbortController | null = null;
  let delay = 1000;

  const run = async () => {
    while (!stopped) {
      controller = new AbortController();
      try {
        const response = await fetch(`http://${node.address}/events`, {
          headers: { Authorization: `Bearer ${node.token}`, Accept: "text/event-stream" },
          signal: controller.signal,
        });
        if (!response.ok || !response.body) throw new Error(`events ${response.status}`);
        delay = 1000;
        reportLink(node.id, "up");
        const reader = response.body.getReader();
        const decoder = new TextDecoder();
        let buffer = "";
        for (;;) {
          const { value, done } = await reader.read();
          if (done) break;
          buffer += decoder.decode(value, { stream: true });
          let index: number;
          while ((index = buffer.indexOf("\n\n")) >= 0) {
            const chunk = buffer.slice(0, index);
            buffer = buffer.slice(index + 2);
            const data = chunk
              .split("\n")
              .filter((line) => line.startsWith("data:"))
              .map((line) => line.slice(5).trimStart())
              .join("\n");
            if (data) {
              try {
                handler(JSON.parse(data));
              } catch {
                // A malformed event is dropped, not fatal.
              }
            }
          }
        }
      } catch {
        // Connection lost: retry below.
      }
      if (stopped) break;
      reportLink(node.id, "down");
      await new Promise((resolve) => window.setTimeout(resolve, delay));
      delay = Math.min(delay * 2, 15000);
    }
  };
  void run();
  return () => {
    stopped = true;
    controller?.abort();
  };
}
