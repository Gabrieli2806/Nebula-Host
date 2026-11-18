/**
 * Build the project, put the jar on its test server, restart it, and say
 * whether it came up: the loop a mod or plugin developer runs all day.
 */

import * as path from "node:path";
import * as vscode from "vscode";

import { NebulaClient, Server, withMachine } from "./client";
import { Consoles } from "./console";
import { buildCommand, builtJar, KIND_LABEL, Project } from "./project";

/** The server a project deploys to, remembered per project folder. */
export interface Target {
  server: string;
  machine?: string;
}

const DIST_LABEL: Record<string, string> = { paper: "Paper", fabric: "Fabric", forge: "Forge", neoforge: "NeoForge" };

export class Deployer {
  constructor(
    private readonly client: NebulaClient,
    private readonly consoles: Consoles,
    private readonly state: vscode.Memento,
    private readonly onChange: () => void,
  ) {}

  target(project: Project): Target | undefined {
    return this.state.get<Target>(`target:${project.root}`);
  }

  private async remember(project: Project, target: Target): Promise<void> {
    await this.state.update(`target:${project.root}`, target);
    this.onChange();
  }

  /** Ask which server this project deploys to; offers to create one. */
  async pickTarget(project: Project, remotes: string[]): Promise<Target | undefined> {
    type Pick = vscode.QuickPickItem & { target?: Target; create?: string };
    const items: Pick[] = [];
    const machines: (string | undefined)[] = [undefined, ...remotes];
    for (const machine of machines) {
      let servers: Server[] = [];
      try {
        servers = await this.client.servers(machine);
      } catch {
        continue;
      }
      const fits = (s: Server) => s.distribution === project.distribution || (project.kind === "plugin" && ["paper", "purpur", "spigot"].includes(s.distribution));
      const sameVersion = (s: Server) => !!project.minecraft && (s.version === project.minecraft || s.version.startsWith(`${project.minecraft}.`));
      const ordered = servers.filter(fits).sort((a, b) => Number(sameVersion(b)) - Number(sameVersion(a)));
      if (ordered.length || machine === undefined) {
        items.push({ label: machine ?? "This PC", kind: vscode.QuickPickItemKind.Separator });
      }
      for (const s of ordered) {
        items.push({
          label: `$(server) ${s.name}`,
          description: `${DIST_LABEL[s.distribution] ?? s.distribution} ${s.version}${sameVersion(s) ? "" : "  (different version)"}`,
          target: { server: s.name, machine },
        });
      }
      items.push({ label: `$(add) Create a test server${machine ? ` on ${machine}` : ""}…`, create: machine ?? "" });
    }
    const choice = await vscode.window.showQuickPick(items, {
      title: `Test server for ${project.name} (${KIND_LABEL[project.kind]}${project.minecraft ? `, Minecraft ${project.minecraft}` : ""})`,
      placeHolder: "Where should builds of this project run?",
    });
    if (!choice) return undefined;
    const target = choice.target ?? (choice.create !== undefined ? await this.createTestServer(project, choice.create || undefined) : undefined);
    if (target) await this.remember(project, target);
    return target;
  }

  /** The Minecraft version to create a server with: the project's, made exact. */
  private async chooseVersion(distribution: string, wanted: string | undefined, machine?: string): Promise<string | undefined> {
    let versions: string[] = [];
    try {
      const text = await this.client.call("list_versions", withMachine({ distribution }, machine));
      versions = text.replace(/^[^:]*:\s*/, "").split(",").map((v) => v.trim()).filter(Boolean);
    } catch {
      return wanted;
    }
    if (wanted) {
      if (versions.includes(wanted)) return wanted;
      const line = versions.find((v) => v.startsWith(`${wanted}.`));
      if (line) return line;
    }
    return vscode.window.showQuickPick(versions, {
      title: wanted ? `Minecraft ${wanted} is not available for ${DIST_LABEL[distribution] ?? distribution}` : "Minecraft version",
      placeHolder: "Version of the test server",
    });
  }

  /** Make a test server the way the workflow names them: `Test Paper 1.21.4`, offline mode. */
  async createTestServer(project?: Project, machine?: string): Promise<Target | undefined> {
    let distribution = project?.distribution;
    if (!distribution) {
      const pick = await vscode.window.showQuickPick(
        [
          { label: "Paper", description: "plugins", id: "paper" },
          { label: "Fabric", description: "mods", id: "fabric" },
          { label: "Forge", description: "mods", id: "forge" },
          { label: "NeoForge", description: "mods", id: "neoforge" },
          { label: "Vanilla", description: "no mods", id: "vanilla" },
        ],
        { title: "Create a test server", placeHolder: "Server type" },
      );
      if (!pick) return undefined;
      distribution = pick.id;
    }
    const version = await this.chooseVersion(distribution, project?.minecraft, machine);
    if (!version) return undefined;
    const name = `Test ${DIST_LABEL[distribution] ?? distribution} ${version}`;
    const memory = vscode.workspace.getConfiguration("nebulaHost").get<number>("testServerMemoryMb") ?? 2048;

    return vscode.window.withProgress(
      { location: vscode.ProgressLocation.Notification, title: `Creating ${name}` },
      async (progress) => {
        const created = await this.client.call(
          "create_server",
          withMachine({ name, distribution, version, memory_mb: memory, settings: { "online-mode": "false" }, start: true }, machine),
        );
        this.onChange();
        progress.report({ message: "Getting ready (downloading the server)…" });
        const waited = await this.client.call("wait_for_server", withMachine({ server: name, until: "online", timeout_seconds: 600 }, machine), 660_000);
        this.onChange();
        if (/not starting|cannot start|EULA/i.test(waited + created)) {
          void vscode.window.showWarningMessage(
            `${name} is installed but cannot start until the Minecraft EULA is accepted in Nebula Host (Settings › Servers).`,
          );
        } else {
          void vscode.window.showInformationMessage(waited);
        }
        return { server: name, machine };
      },
    );
  }

  /** Run the build in a terminal task; true when it succeeded. */
  private async build(project: Project): Promise<boolean> {
    const command = buildCommand(project);
    if (!command) return true;
    const task = new vscode.Task(
      { type: "shell", task: "nebula-build" },
      vscode.TaskScope.Workspace,
      `Build ${project.name}`,
      "Nebula Host",
      new vscode.ShellExecution(command, { cwd: project.root }),
      [],
    );
    task.presentationOptions = { reveal: vscode.TaskRevealKind.Silent, panel: vscode.TaskPanelKind.Dedicated, clear: true };
    const execution = await vscode.tasks.executeTask(task);
    const code = await new Promise<number | undefined>((resolve) => {
      const listener = vscode.tasks.onDidEndTaskProcess((event) => {
        if (event.execution === execution) {
          listener.dispose();
          resolve(event.exitCode);
        }
      });
    });
    if (code !== 0) {
      const show = await vscode.window.showErrorMessage(`The build of ${project.name} failed (exit code ${code ?? "?"}).`, "Show build output");
      // The build ran in its own terminal, the most recent one.
      if (show) void vscode.commands.executeCommand("workbench.action.terminal.focus");
      return false;
    }
    return true;
  }

  /** Build (optionally), install the jar on the target and restart it. */
  async deploy(project: Project, options: { build: boolean; target?: Target; remotes: string[] }): Promise<void> {
    const target = options.target ?? this.target(project) ?? (await this.pickTarget(project, options.remotes));
    if (!target) return;

    if (options.build && !(await this.build(project))) return;

    const jar = builtJar(project);
    if (!jar) {
      void vscode.window.showErrorMessage(`No built jar in ${project.name}'s build/libs or target folder. Build it first.`);
      return;
    }

    await vscode.window.withProgress(
      { location: vscode.ProgressLocation.Notification, title: `Deploying ${path.basename(jar)} to ${target.server}` },
      async (progress) => {
        const previous = this.state.get<string>(`jar:${project.root}:${target.machine ?? ""}/${target.server}`);
        const args: Record<string, unknown> = { server: target.server, path: jar, restart: true, start: true };
        if (previous && previous !== path.basename(jar)) args.replace = previous;
        try {
          await this.client.call("install_local_addon", withMachine(args, target.machine), 180_000);
        } catch (error) {
          const message = error instanceof Error ? error.message : String(error);
          if (/instance_not_found/.test(message)) {
            await this.state.update(`target:${project.root}`, undefined);
            void vscode.window.showErrorMessage(`${target.server} no longer exists. Choose another test server and deploy again.`);
            return;
          }
          throw error;
        }
        await this.state.update(`jar:${project.root}:${target.machine ?? ""}/${target.server}`, path.basename(jar));
        this.onChange();

        if (vscode.workspace.getConfiguration("nebulaHost").get<boolean>("followConsoleAfterDeploy")) {
          this.consoles.open(target.server, target.machine, true);
        }
        progress.report({ message: "Starting the server…" });
        const waited = await this.client.call("wait_for_server", withMachine({ server: target.server, until: "online", timeout_seconds: 300 }, target.machine), 330_000);
        this.onChange();

        if (/CRASHED/.test(waited)) {
          const action = await vscode.window.showErrorMessage(`${target.server} crashed while starting with ${path.basename(jar)}.`, "Diagnose", "Open console");
          if (action === "Diagnose") await showDiagnosis(this.client, target.server, target.machine);
          if (action === "Open console") this.consoles.open(target.server, target.machine);
          return;
        }
        if (!/is online/.test(waited)) {
          void vscode.window.showWarningMessage(waited);
          return;
        }
        const problems = await this.problemsAbout(project, target);
        if (problems.length) {
          const action = await vscode.window.showWarningMessage(
            `${target.server} is online, with ${problems.length} warning(s) or error(s) mentioning ${project.name}.`,
            "Open console",
          );
          if (action) this.consoles.open(target.server, target.machine);
        } else {
          void vscode.window.showInformationMessage(`${waited.replace(/\.$/, "")} — ${path.basename(jar)} loaded with no problems logged.`);
        }
      },
    );
  }

  /** Warnings and errors that name the project, from the console since it started. */
  private async problemsAbout(project: Project, target: Target): Promise<string[]> {
    try {
      const text = await this.client.call("get_console", withMachine({ server: target.server, lines: 400, only_problems: true }, target.machine));
      const needle = project.name.toLowerCase().replace(/[-_ ]/g, "");
      return text
        .split("\n")
        .filter((line) => line.toLowerCase().replace(/[-_ ]/g, "").includes(needle));
    } catch {
      return [];
    }
  }
}

/** The app's diagnosis of a server, as a Markdown document. */
export async function showDiagnosis(client: NebulaClient, server: string, machine?: string): Promise<void> {
  const text = await vscode.window.withProgress(
    { location: vscode.ProgressLocation.Notification, title: `Diagnosing ${server}` },
    () => client.call("diagnose_server", withMachine({ server }, machine)),
  );
  const document = await vscode.workspace.openTextDocument({ language: "markdown", content: `# ${server}\n\n${text}` });
  await vscode.window.showTextDocument(document, { preview: true });
}
