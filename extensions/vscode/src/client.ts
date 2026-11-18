/**
 * Nebula Host's agent endpoint (MCP over HTTP, JSON answers), from VS Code.
 *
 * The same tools an AI agent uses: one door into the app, with its rules
 * (read-only, paired machines, the EULA) applied by the app itself. No token:
 * the endpoint only answers programs on this PC.
 */

import * as vscode from "vscode";

export class NebulaError extends Error {
  constructor(
    message: string,
    /** The endpoint did not answer at all (app closed, agent access off). */
    readonly unreachable = false,
  ) {
    super(message);
  }
}

/** A server as `list_servers` describes it. */
export interface Server {
  id: string;
  name: string;
  status: "offline" | "sleeping" | "starting" | "online" | "stopping" | "crashed";
  distribution: string;
  version: string;
  loader_version?: string | null;
  players_online: number;
  players_max: number;
  memory_allocated_mb: number;
  memory_used_mb: number;
  cpu_percent: number;
  address?: string | null;
  installed: boolean;
  /** Install phase while it is being set up, or `failed`. */
  setup?: string | null;
}

export interface Machine {
  name: string;
  this_pc?: boolean;
  address?: string;
  reachable?: boolean;
  allows_agents?: boolean | string;
}

let nextId = 1;

export class NebulaClient {
  private initialized = false;

  private get endpoint(): string {
    return vscode.workspace.getConfiguration("nebulaHost").get<string>("endpoint") || "http://127.0.0.1:47391/mcp";
  }

  private async rpc(method: string, params: unknown, timeoutMs: number): Promise<unknown> {
    let response: Response;
    try {
      response = await fetch(this.endpoint, {
        method: "POST",
        headers: { "Content-Type": "application/json", Accept: "application/json" },
        body: JSON.stringify({ jsonrpc: "2.0", id: nextId++, method, params }),
        signal: AbortSignal.timeout(timeoutMs),
      });
    } catch (error) {
      this.initialized = false;
      const timedOut = error instanceof Error && error.name === "TimeoutError";
      throw new NebulaError(timedOut ? "Nebula Host took too long to answer." : "Nebula Host is not answering.", !timedOut);
    }
    if (!response.ok) {
      throw new NebulaError(`Nebula Host refused the request (${response.status}): ${await response.text()}`);
    }
    const body = (await response.json()) as { result?: unknown; error?: { message: string } };
    if (body.error) throw new NebulaError(body.error.message);
    return body.result;
  }

  /** Say who is calling, once per connection: the app shows it as the last agent. */
  private async ensureInitialized(): Promise<void> {
    if (this.initialized) return;
    await this.rpc(
      "initialize",
      {
        protocolVersion: "2025-06-18",
        capabilities: {},
        clientInfo: { name: "VS Code (Nebula Host extension)", version: "0.1.0" },
      },
      5000,
    );
    this.initialized = true;
  }

  /** Run a tool; its text, or an error carrying the app's explanation. */
  async call(tool: string, args: Record<string, unknown> = {}, timeoutMs = 60_000): Promise<string> {
    await this.ensureInitialized();
    const result = (await this.rpc("tools/call", { name: tool, arguments: args }, timeoutMs)) as {
      content?: { type: string; text?: string }[];
      isError?: boolean;
    };
    const text = (result.content ?? []).map((part) => part.text ?? "").join("\n");
    if (result.isError) throw new NebulaError(text.replace(/^Error: /, ""));
    return text;
  }

  /** A tool whose answer is JSON (or a sentence when there is nothing). */
  async callJson<T>(tool: string, args: Record<string, unknown> = {}, fallback: T): Promise<T> {
    const text = await this.call(tool, args);
    try {
      return JSON.parse(text) as T;
    } catch {
      return fallback;
    }
  }

  servers(machine?: string): Promise<Server[]> {
    return this.callJson<Server[]>("list_servers", withMachine({}, machine), []);
  }

  machines(): Promise<Machine[]> {
    return this.callJson<Machine[]>("list_machines", {}, []);
  }
}

/** Add `machine` to a tool's arguments when it is not this PC. */
export function withMachine(args: Record<string, unknown>, machine?: string): Record<string, unknown> {
  return machine ? { ...args, machine } : args;
}
