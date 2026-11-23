/**
 * Nebula Host for VS Code: the test servers of the Nebula Host app, from the
 * editor. Optional on both sides — the app works without it, and it does
 * nothing until the app's agent access is on.
 *
 * - Servers view: this PC's servers and those of paired machines, start, stop,
 *   console, diagnosis.
 * - Build and deploy (Ctrl+Alt+D): build the open plugin or mod, put the jar
 *   on its test server, restart it, say whether it loaded.
 * - Registers Nebula Host's MCP tools with VS Code, so Copilot's agent mode
 *   and other chat extensions can use them without setup.
 */

import * as vscode from "vscode";

import { NebulaClient, NebulaError, withMachine } from "./client";
import { Consoles } from "./console";
import { Deployer, showDiagnosis, Target } from "./deploy";
import { detectProject, KIND_LABEL, Project } from "./project";
import { isRunning, ServerNode, ServersTree } from "./tree";

export function activate(context: vscode.ExtensionContext): void {
  const client = new NebulaClient();
  const consoles = new Consoles(client);
  const tree = new ServersTree(client);
  const view = vscode.window.createTreeView("nebulaHost.servers", { treeDataProvider: tree, showCollapseAll: false });
  const status = vscode.window.createStatusBarItem("nebulaHost.status", vscode.StatusBarAlignment.Left, 50);
  status.name = "Nebula Host";
  context.subscriptions.push(consoles, view, status);

  let project: Project | undefined;
  let state: "online" | "offline" | "empty" | undefined;

  const deployer = new Deployer(client, consoles, context.workspaceState, () => {
    tree.refresh();
    updateStatus();
  });

  function updateStatus(): void {
    if (!project) {
      status.hide();
      return;
    }
    const target = deployer.target(project);
    if (state === "offline") {
      status.text = "$(circle-slash) Nebula Host";
      status.tooltip = "Nebula Host is not answering. Open the app and turn on Settings › AI agents (MCP).";
      status.command = "nebulaHost.refresh";
    } else {
      status.text = `$(rocket) ${target ? target.server : "Nebula Host"}`;
      status.tooltip = target
        ? `Build ${project.name} and deploy it to ${target.server}${target.machine ? ` on ${target.machine}` : ""} (Ctrl+Alt+D)`
        : `Choose a test server for ${project.name} (${KIND_LABEL[project.kind]})`;
      status.command = target ? "nebulaHost.buildAndDeploy" : "nebulaHost.pickTestServer";
    }
    status.show();
  }

  function detect(): void {
    project = detectProject();
    void vscode.commands.executeCommand("setContext", "nebulaHost.project", project !== undefined);
    updateStatus();
  }

  tree.onState = (next) => {
    if (next === state) return;
    state = next;
    void vscode.commands.executeCommand("setContext", "nebulaHost.state", next);
    updateStatus();
  };

  detect();
  context.subscriptions.push(
    vscode.window.onDidChangeActiveTextEditor(() => detect()),
    vscode.workspace.onDidChangeWorkspaceFolders(() => detect()),
  );

  // Refresh while someone looks: the view, or the status item of a project.
  let timer: NodeJS.Timeout | undefined;
  const schedule = () => {
    if (timer) clearInterval(timer);
    const seconds = Math.max(2, vscode.workspace.getConfiguration("nebulaHost").get<number>("refreshSeconds") ?? 5);
    timer = setInterval(() => {
      if (view.visible) {
        tree.refresh();
      } else if (project) {
        // Only whether the app answers, for the status item.
        client.servers().then(
          (servers) => tree.onState?.(servers.length ? "online" : "empty"),
          (error) => {
            if (error instanceof NebulaError && error.unreachable) tree.onState?.("offline");
          },
        );
      }
    }, seconds * 1000);
  };
  schedule();
  context.subscriptions.push({ dispose: () => timer && clearInterval(timer) });
  context.subscriptions.push(
    vscode.workspace.onDidChangeConfiguration((event) => {
      if (event.affectsConfiguration("nebulaHost.refreshSeconds")) schedule();
      if (event.affectsConfiguration("nebulaHost")) mcpChanged.fire();
      if (event.affectsConfiguration("nebulaHost.endpoint")) tree.refresh();
    }),
  );

  // -- MCP for VS Code's own agents ---------------------------------------
  const mcpChanged = new vscode.EventEmitter<void>();
  context.subscriptions.push(mcpChanged);
  if (typeof vscode.lm?.registerMcpServerDefinitionProvider === "function") {
    context.subscriptions.push(
      vscode.lm.registerMcpServerDefinitionProvider("nebula-host.mcp", {
        onDidChangeMcpServerDefinitions: mcpChanged.event,
        provideMcpServerDefinitions: () => {
          const config = vscode.workspace.getConfiguration("nebulaHost");
          if (!config.get<boolean>("registerMcpServer")) return [];
          const endpoint = config.get<string>("endpoint") || "http://127.0.0.1:47391/mcp";
          return [new vscode.McpHttpServerDefinition("Nebula Host", vscode.Uri.parse(endpoint), {}, "0.1.0")];
        },
      }),
    );
  }

  // -- commands ------------------------------------------------------------

  /** The server a command is about: the tree node it came from, or a pick. */
  async function serverFor(node: unknown, running?: boolean): Promise<ServerNode | undefined> {
    if (node instanceof ServerNode) return node;
    if (!tree.known.length) await tree.getChildren();
    const nodes = tree.known.filter((n) => running === undefined || isRunning(n.server) === running);
    if (!nodes.length) {
      void vscode.window.showInformationMessage(running ? "No server is running." : "No servers found.");
      return undefined;
    }
    const pick = await vscode.window.showQuickPick(
      nodes.map((n) => ({ label: n.server.name, description: `${n.server.status}${n.machine ? ` · ${n.machine}` : ""}`, node: n })),
      { placeHolder: "Server" },
    );
    return pick?.node;
  }

  /** Run a tool for a server and refresh; errors become a message. */
  async function act(node: ServerNode, tool: string, done?: string): Promise<void> {
    try {
      const text = await client.call(tool, withMachine({ server: node.server.name }, node.machine));
      if (done) vscode.window.setStatusBarMessage(`$(check) ${done}`, 4000);
      else vscode.window.setStatusBarMessage(text, 4000);
    } catch (error) {
      void vscode.window.showErrorMessage(friendly(error));
    }
    tree.refresh();
  }

  function needProject(): Project | undefined {
    detect();
    if (!project) {
      void vscode.window.showWarningMessage(
        "No plugin or mod project here: Nebula Host looks for plugin.yml, fabric.mod.json, mods.toml or neoforge.mods.toml under src/main/resources.",
      );
    }
    return project;
  }

  const register = (id: string, run: (...args: unknown[]) => unknown) =>
    context.subscriptions.push(
      vscode.commands.registerCommand(id, async (...args: unknown[]) => {
        try {
          await run(...args);
        } catch (error) {
          void vscode.window.showErrorMessage(friendly(error));
        }
      }),
    );

  register("nebulaHost.refresh", () => tree.refresh());
  register("nebulaHost.openSettings", () => vscode.commands.executeCommand("workbench.action.openSettings", "nebulaHost"));

  register("nebulaHost.buildAndDeploy", async () => {
    const p = needProject();
    if (p) await deployer.deploy(p, { build: true, remotes: tree.remotes });
  });
  register("nebulaHost.deploy", async () => {
    const p = needProject();
    if (p) await deployer.deploy(p, { build: false, remotes: tree.remotes });
  });
  register("nebulaHost.deployHere", async (node) => {
    const p = needProject();
    const target = await serverFor(node);
    if (!p || !target) return;
    const chosen: Target = { server: target.server.name, machine: target.machine };
    await deployer.deploy(p, { build: true, target: chosen, remotes: tree.remotes });
  });
  register("nebulaHost.pickTestServer", async () => {
    const p = needProject();
    if (p) await deployer.pickTarget(p, tree.remotes);
  });
  register("nebulaHost.createTestServer", async () => {
    detect();
    await deployer.createTestServer(project);
    tree.refresh();
  });

  register("nebulaHost.start", async (node) => {
    const n = await serverFor(node, false);
    if (n) await act(n, "start_server", `Starting ${n.server.name}`);
  });
  register("nebulaHost.stop", async (node) => {
    const n = await serverFor(node, true);
    if (n) await act(n, "stop_server", `Stopping ${n.server.name}`);
  });
  register("nebulaHost.restart", async (node) => {
    const n = await serverFor(node, true);
    if (n) await act(n, "restart_server", `Restarting ${n.server.name}`);
  });
  register("nebulaHost.console", async (node) => {
    const n = await serverFor(node);
    if (n) consoles.open(n.server.name, n.machine);
  });
  register("nebulaHost.sendCommand", async (node) => {
    const n = await serverFor(node, true);
    if (!n) return;
    const command = await vscode.window.showInputBox({
      title: `Console command on ${n.server.name}`,
      prompt: "Without the slash, e.g. `op Dev` or `myplugin reload`",
      placeHolder: "say hello",
    });
    if (!command) return;
    consoles.open(n.server.name, n.machine, true);
    const output = await client.call("send_command", withMachine({ server: n.server.name, command }, n.machine));
    vscode.window.setStatusBarMessage(`$(terminal) ${output.split("\n")[0]}`, 5000);
  });
  register("nebulaHost.diagnose", async (node) => {
    const n = await serverFor(node);
    if (n) await showDiagnosis(client, n.server.name, n.machine);
  });
  register("nebulaHost.copyAddress", async (node) => {
    const n = await serverFor(node);
    if (!n) return;
    if (!n.server.address) {
      void vscode.window.showInformationMessage(`${n.server.name} has no address yet.`);
      return;
    }
    await vscode.env.clipboard.writeText(n.server.address);
    vscode.window.setStatusBarMessage(`$(copy) Copied ${n.server.address}`, 3000);
  });
  register("nebulaHost.openFolder", async (node) => {
    const n = await serverFor(node);
    if (!n || n.machine) return;
    const details = JSON.parse(await client.call("get_server", { server: n.server.name })) as { path?: string };
    if (details.path) await vscode.env.openExternal(vscode.Uri.file(details.path));
  });
}

/** An error the way a person should read it. */
function friendly(error: unknown): string {
  if (error instanceof NebulaError && error.unreachable) {
    return "Nebula Host is not answering. Open the app and turn on Settings › AI agents (MCP) › Let AI agents use Nebula Host.";
  }
  const text = error instanceof Error ? error.message : String(error);
  if (/agreement_required/.test(text)) return "This server needs the Minecraft EULA accepted in Nebula Host first (Settings › Servers, or on the server's page).";
  if (/read-only|only look/i.test(text)) return "Nebula Host lets agents look but not change anything. Turn off View only in Settings › AI agents (MCP) › View details.";
  return text;
}

export function deactivate(): void {
  // Disposables do the work.
}
