/**
 * What the open project is: a plugin or a mod, for which loader and Minecraft
 * version, how it builds, and which jar the build makes.
 */

import * as fs from "node:fs";
import * as path from "node:path";
import * as vscode from "vscode";

export type Kind = "plugin" | "fabric" | "forge" | "neoforge";

export interface Project {
  root: string;
  name: string;
  kind: Kind;
  /** Minecraft version the project targets, when it says. */
  minecraft?: string;
  /** The distribution a test server for it runs. */
  distribution: string;
  build: "gradle" | "maven" | "none";
}

const RESOURCES = path.join("src", "main", "resources");

function exists(...parts: string[]): boolean {
  return fs.existsSync(path.join(...parts));
}

function read(file: string): string {
  try {
    return fs.readFileSync(file, "utf8");
  } catch {
    return "";
  }
}

/** `key=value` from gradle.properties. */
function gradleProperty(root: string, keys: string[]): string | undefined {
  const text = read(path.join(root, "gradle.properties"));
  for (const key of keys) {
    const match = text.match(new RegExp(`^\\s*${key.replace(".", "\\.")}\\s*=\\s*(.+?)\\s*$`, "m"));
    if (match) return match[1];
  }
  return undefined;
}

function detectAt(root: string): Project | undefined {
  const resources = path.join(root, RESOURCES);
  const build: Project["build"] = exists(root, "gradlew") || exists(root, "gradlew.bat") || exists(root, "build.gradle") || exists(root, "build.gradle.kts")
    ? "gradle"
    : exists(root, "pom.xml")
      ? "maven"
      : "none";
  const mcFromGradle = gradleProperty(root, ["minecraft_version", "mc_version", "minecraftVersion", "minecraft.version"]);
  const name = gradleProperty(root, ["archives_base_name", "mod_id", "rootProject.name"]) ?? path.basename(root);

  if (exists(resources, "fabric.mod.json")) {
    return { root, name, kind: "fabric", distribution: "fabric", minecraft: mcFromGradle, build };
  }
  if (exists(resources, "META-INF", "neoforge.mods.toml")) {
    return { root, name, kind: "neoforge", distribution: "neoforge", minecraft: mcFromGradle, build };
  }
  if (exists(resources, "META-INF", "mods.toml")) {
    return { root, name, kind: "forge", distribution: "forge", minecraft: mcFromGradle, build };
  }
  for (const file of ["plugin.yml", "paper-plugin.yml"]) {
    if (exists(resources, file)) {
      const yml = read(path.join(resources, file));
      const api = yml.match(/^api-version:\s*['"]?([\d.]+)/m)?.[1];
      const pluginName = yml.match(/^name:\s*['"]?([^'"\r\n]+)/m)?.[1]?.trim();
      return {
        root,
        name: pluginName ?? name,
        kind: "plugin",
        distribution: "paper",
        // api-version is the minor line (1.21); gradle may say the exact one.
        minecraft: mcFromGradle ?? api,
        build,
      };
    }
  }
  return undefined;
}

/** The project in the workspace: the folder of the active file first, then each workspace folder. */
export function detectProject(): Project | undefined {
  const candidates: string[] = [];
  const active = vscode.window.activeTextEditor?.document.uri;
  if (active?.scheme === "file") {
    const folder = vscode.workspace.getWorkspaceFolder(active);
    if (folder) candidates.push(folder.uri.fsPath);
  }
  for (const folder of vscode.workspace.workspaceFolders ?? []) candidates.push(folder.uri.fsPath);
  for (const root of new Set(candidates)) {
    const project = detectAt(root);
    if (project) return project;
  }
  return undefined;
}

export const KIND_LABEL: Record<Kind, string> = {
  plugin: "Paper plugin",
  fabric: "Fabric mod",
  forge: "Forge mod",
  neoforge: "NeoForge mod",
};

/** What builds the jar, as a shell command run in the project folder. */
export function buildCommand(project: Project): string | undefined {
  const custom = vscode.workspace.getConfiguration("nebulaHost").get<string>("buildCommand")?.trim();
  if (custom) return custom;
  if (project.build === "gradle") {
    if (process.platform === "win32" && exists(project.root, "gradlew.bat")) return ".\\gradlew.bat build";
    if (exists(project.root, "gradlew")) return "./gradlew build";
    return "gradle build";
  }
  if (project.build === "maven") return "mvn -q package";
  return undefined;
}

/** Jars a build leaves that are not the one to run. */
const NOT_THE_JAR = /-(sources|javadoc|dev|dev-shadow|api|slim)\.jar$|^original-/i;

/**
 * The jar the last build made: the newest runnable jar in build/libs (Gradle)
 * or target (Maven). A shaded `-all` jar wins over the plain one, since that
 * is the one that carries its dependencies.
 */
export function builtJar(project: Project): string | undefined {
  const dirs = [path.join(project.root, "build", "libs"), path.join(project.root, "target")];
  const jars: { file: string; mtime: number }[] = [];
  for (const dir of dirs) {
    let names: string[] = [];
    try {
      names = fs.readdirSync(dir);
    } catch {
      continue;
    }
    for (const name of names) {
      if (!name.toLowerCase().endsWith(".jar") || NOT_THE_JAR.test(name)) continue;
      const file = path.join(dir, name);
      jars.push({ file, mtime: fs.statSync(file).mtimeMs });
    }
  }
  if (!jars.length) return undefined;
  jars.sort((a, b) => b.mtime - a.mtime);
  // Builds of the same run land within seconds of each other.
  const latest = jars.filter((jar) => jars[0].mtime - jar.mtime < 60_000);
  return (latest.find((jar) => /-all\.jar$/i.test(jar.file)) ?? latest[0]).file;
}
