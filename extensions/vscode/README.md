# Nebula Host for VS Code

Test Minecraft plugins and mods on your Nebula Host servers without leaving the editor: build, put the jar on a test server, restart it and see whether it loaded.

## Requirements

- Nebula Host running on this PC, with **Settings › AI agents (MCP) › Let AI agents use Nebula Host** turned on.
- A Paper/Spigot plugin (`plugin.yml`) or a Fabric, Forge or NeoForge mod (`fabric.mod.json`, `mods.toml`, `neoforge.mods.toml`) built with Gradle or Maven.

## How it works

The extension talks to the Nebula Host app through its local agent endpoint (`http://127.0.0.1:47391/mcp`), with the same tools an AI agent uses. The app keeps its own rules: view-only mode, the EULA setting, and which paired machines let agents in. The extension does nothing when the app is closed.

## Features

| Feature | What it does |
|---------|--------------|
| Servers view | Servers on this PC and on paired machines that allow agents: status, version, players, address. Start, stop, restart, copy the address, open the folder. |
| Build and Deploy (`Ctrl+Alt+D`) | Runs `gradlew build` (or Maven), copies the jar into the test server's `plugins` or `mods` folder (replacing the previous build), restarts the server and reports whether it came up and whether the console logged problems that name the project. |
| Test server | Chosen once per project, remembered. **Create Test Server** makes `Test Paper 1.21.4` (or Fabric, Forge, NeoForge) in offline mode, so a `runClient` dev client can join. |
| Console | The server's console in the Output panel, following it while it runs. **Run Console Command…** sends a command. |
| Diagnose | Nebula Host's diagnosis of a crash (errors, crash report, known problems) as a Markdown document. |
| AI agents | Nebula Host's MCP tools are offered to Copilot's agent mode and other chat extensions in VS Code, with no setup. |

## Settings

| Setting | Default | Meaning |
|---------|---------|---------|
| `nebulaHost.endpoint` | `http://127.0.0.1:47391/mcp` | The app's agent endpoint. |
| `nebulaHost.registerMcpServer` | `true` | Offer Nebula Host's tools to AI agents in VS Code. |
| `nebulaHost.buildCommand` | empty | Build command. Empty: Gradle wrapper, then Maven. |
| `nebulaHost.followConsoleAfterDeploy` | `true` | Open the console after a deploy. |
| `nebulaHost.testServerMemoryMb` | `2048` | Memory for test servers the extension creates. |
| `nebulaHost.refreshSeconds` | `5` | How often the Servers view refreshes while visible. |

## Build the extension

```powershell
cd extensions/vscode
npm install
npm run package
code --install-extension nebula-host.vsix
```
