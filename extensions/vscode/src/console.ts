/**
 * A server's console in VS Code's Output panel, following along while the
 * server runs. The endpoint has no stream, so it asks for the tail every
 * second and adds the lines it has not shown yet.
 */

import * as vscode from "vscode";

import { NebulaClient, withMachine } from "./client";

const POLL_MS = 1200;
/** Stop asking once the server has been stopped for this long. */
const IDLE_STOP_MS = 15_000;

class Follower {
  readonly channel: vscode.OutputChannel;
  private shown: string[] = [];
  private timer: NodeJS.Timeout | undefined;
  private stoppedSince: number | undefined;
  private busy = false;

  constructor(
    private readonly client: NebulaClient,
    readonly server: string,
    readonly machine: string | undefined,
  ) {
    const where = machine ? ` (${machine})` : "";
    this.channel = vscode.window.createOutputChannel(`Nebula: ${server}${where}`, "log");
  }

  start(): void {
    this.stoppedSince = undefined;
    if (!this.timer) {
      void this.tick();
      this.timer = setInterval(() => void this.tick(), POLL_MS);
    }
  }

  stop(): void {
    if (this.timer) clearInterval(this.timer);
    this.timer = undefined;
  }

  dispose(): void {
    this.stop();
    this.channel.dispose();
  }

  private async tick(): Promise<void> {
    if (this.busy) return;
    this.busy = true;
    try {
      const text = await this.client.call("get_console", withMachine({ server: this.server, lines: 300 }, this.machine), 10_000);
      const lines = text.startsWith("The console is empty") ? [] : text.split("\n");
      this.append(lines);
      await this.checkRunning();
    } catch (error) {
      this.channel.appendLine(`[nebula] ${error instanceof Error ? error.message : String(error)}`);
      this.stop();
    } finally {
      this.busy = false;
    }
  }

  /** Add what is new: the longest end of what is shown that the tail starts with is already there. */
  private append(lines: string[]): void {
    if (!lines.length) return;
    let overlap = Math.min(this.shown.length, lines.length);
    while (overlap > 0) {
      const tail = this.shown.slice(this.shown.length - overlap);
      if (tail.every((line, i) => line === lines[i])) break;
      overlap -= 1;
    }
    if (overlap === 0 && this.shown.length) this.channel.appendLine("────────");
    const fresh = lines.slice(overlap);
    if (fresh.length) this.channel.append(fresh.join("\n") + "\n");
    this.shown = lines;
  }

  private async checkRunning(): Promise<void> {
    const servers = await this.client.servers(this.machine);
    const server = servers.find((s) => s.name === this.server || s.id === this.server);
    const running = server && (server.status === "online" || server.status === "starting" || server.status === "stopping");
    if (running) {
      this.stoppedSince = undefined;
      return;
    }
    this.stoppedSince ??= Date.now();
    if (Date.now() - this.stoppedSince > IDLE_STOP_MS) this.stop();
  }
}

export class Consoles implements vscode.Disposable {
  private readonly followers = new Map<string, Follower>();

  constructor(private readonly client: NebulaClient) {}

  /** Show a server's console and follow it. */
  open(server: string, machine?: string, preserveFocus = false): void {
    const key = `${machine ?? ""}/${server}`;
    let follower = this.followers.get(key);
    if (!follower) {
      follower = new Follower(this.client, server, machine);
      this.followers.set(key, follower);
    }
    follower.channel.show(preserveFocus);
    follower.start();
  }

  dispose(): void {
    for (const follower of this.followers.values()) follower.dispose();
    this.followers.clear();
  }
}
