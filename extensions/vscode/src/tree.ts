/**
 * The Servers view: this PC's servers, and those of the paired machines that
 * let agents in, each with its status, build and address.
 */

import * as vscode from "vscode";

import { Machine, NebulaClient, NebulaError, Server } from "./client";

export class MachineNode {
  constructor(readonly machine: Machine) {}
}

export class ServerNode {
  constructor(
    readonly server: Server,
    /** The paired machine it runs on; undefined for this PC. */
    readonly machine?: string,
  ) {}
}

export class MessageNode {
  constructor(readonly text: string, readonly detail?: string) {}
}

type Node = MachineNode | ServerNode | MessageNode;

const STATUS_ICON: Record<Server["status"], [string, string]> = {
  online: ["pass-filled", "testing.iconPassed"],
  starting: ["loading~spin", "charts.yellow"],
  stopping: ["loading~spin", "charts.yellow"],
  sleeping: ["debug-pause", "charts.blue"],
  crashed: ["error", "testing.iconFailed"],
  offline: ["circle-large-outline", "disabledForeground"],
};

const STATUS_WORD: Record<Server["status"], string> = {
  online: "Online",
  starting: "Starting",
  stopping: "Stopping",
  sleeping: "Sleeping",
  crashed: "Crashed",
  offline: "Offline",
};

const SETUP_WORD: Record<string, string> = {
  resolving: "Getting ready",
  downloading: "Downloading",
  installing_loader: "Installing the loader",
  configuring: "Configuring",
  generating_world: "Generating the world",
  failed: "Setup failed",
};

export function isRunning(server: Server): boolean {
  return server.status === "online" || server.status === "starting";
}

export class ServersTree implements vscode.TreeDataProvider<Node> {
  private readonly changed = new vscode.EventEmitter<Node | undefined>();
  readonly onDidChangeTreeData = this.changed.event;

  /** The servers last seen, for commands that pick one. */
  known: ServerNode[] = [];
  /** Paired machines that let agents in, last seen. */
  remotes: string[] = [];
  /** Called with whether the app answered. */
  onState?: (state: "online" | "offline" | "empty") => void;

  constructor(private readonly client: NebulaClient) {}

  refresh(): void {
    this.changed.fire(undefined);
  }

  getTreeItem(node: Node): vscode.TreeItem {
    if (node instanceof MessageNode) {
      const item = new vscode.TreeItem(node.text);
      item.description = node.detail;
      item.iconPath = new vscode.ThemeIcon("info");
      return item;
    }
    if (node instanceof MachineNode) {
      const m = node.machine;
      const item = new vscode.TreeItem(m.name, vscode.TreeItemCollapsibleState.Expanded);
      item.iconPath = new vscode.ThemeIcon(m.this_pc ? "device-desktop" : "remote");
      item.description = m.this_pc ? "this PC" : m.address;
      item.contextValue = "machine";
      return item;
    }
    const s = node.server;
    const item = new vscode.TreeItem(s.name);
    const [icon, colour] = s.setup ? (s.setup === "failed" ? STATUS_ICON.crashed : STATUS_ICON.starting) : STATUS_ICON[s.status];
    item.iconPath = new vscode.ThemeIcon(icon, new vscode.ThemeColor(colour));
    const word = s.setup ? SETUP_WORD[s.setup] ?? "Getting ready" : STATUS_WORD[s.status];
    const players = s.status === "online" ? ` · ${s.players_online}/${s.players_max}` : "";
    item.description = `${word}${players} · ${capitalise(s.distribution)} ${s.version}`;
    const tooltip = new vscode.MarkdownString(
      [
        `**${s.name}** — ${word}`,
        "",
        `${capitalise(s.distribution)} ${s.version}${s.loader_version ? ` (${s.loader_version})` : ""}`,
        s.address ? `Address: \`${s.address}\`` : undefined,
        isRunning(s) ? `Memory: ${s.memory_used_mb} / ${s.memory_allocated_mb} MB · CPU ${s.cpu_percent}%` : `Memory: ${s.memory_allocated_mb} MB`,
        node.machine ? `Machine: ${node.machine}` : undefined,
      ]
        .filter((line) => line !== undefined)
        .join("\n\n"),
    );
    item.tooltip = tooltip;
    const state = s.setup ? "setup" : isRunning(s) || s.status === "stopping" ? "running" : "stopped";
    item.contextValue = `server-${state}${node.machine ? "-remote" : "-local"}`;
    item.command = { command: "nebulaHost.console", title: "Open Console", arguments: [node] };
    return item;
  }

  async getChildren(node?: Node): Promise<Node[]> {
    if (node instanceof MachineNode) {
      const machine = node.machine.this_pc ? undefined : node.machine.name;
      try {
        return this.serversOf(await this.client.servers(machine), machine);
      } catch (error) {
        return [new MessageNode("Not available", error instanceof Error ? error.message : String(error))];
      }
    }
    if (node) return [];

    try {
      const [servers, machines] = await Promise.all([this.client.servers(), this.client.machines().catch(() => [] as Machine[])]);
      const others = machines.filter((m) => !m.this_pc && m.reachable && m.allows_agents === true);
      this.remotes = others.map((m) => m.name);
      this.known = this.known.filter((n) => n.machine && this.remotes.includes(n.machine));
      const local = this.serversOf(servers, undefined);
      this.onState?.(servers.length || others.length ? "online" : "empty");
      if (!others.length) return local;
      const here = machines.find((m) => m.this_pc) ?? { name: "This PC", this_pc: true };
      return [new MachineNode(here), ...others.map((m) => new MachineNode(m))];
    } catch (error) {
      this.known = [];
      if (error instanceof NebulaError && error.unreachable) {
        this.onState?.("offline");
        return [];
      }
      this.onState?.("online");
      return [new MessageNode("Nebula Host answered with an error", error instanceof Error ? error.message : String(error))];
    }
  }

  private serversOf(servers: Server[], machine: string | undefined): ServerNode[] {
    const nodes = servers.map((server) => new ServerNode(server, machine));
    this.known = [...this.known.filter((n) => n.machine !== machine), ...nodes];
    return nodes;
  }
}

function capitalise(word: string): string {
  const names: Record<string, string> = { neoforge: "NeoForge", purpur: "Purpur", paper: "Paper", spigot: "Spigot" };
  return names[word] ?? word.charAt(0).toUpperCase() + word.slice(1);
}
