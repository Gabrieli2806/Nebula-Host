// Demo data for store screenshots. Fake names only.
(() => {
  const now = Date.now();
  const iso = (m) => new Date(now - m * 60000).toISOString();
  const mk = (id, name, game, dist, version, status, players, max, cpu, mem, up, extra = {}) => ({
    id, node_id: "n1", game, name, build: { distribution: dist, version }, track_latest_version: false,
    path: `C:\\Servers\\${name}`, runtime_kind: "host_process",
    resources: { memory_mb: 4096, extra_args: [] },
    automation: { sleep_when_empty: false, sleep_after_minutes: 10, wake_on_join: false, restart_after_crash: true,
      auto_backup_hours: 6, auto_backup_keep: 8, restart_attempts_remaining: 3, schedules: [] },
    ports: { game: 25565 }, install: { java_major: 21 }, icon: null, created_at: iso(60 * 24 * 40),
    last_started_at: iso(up), generation: 1, transfer: { state: "active" }, forked_from: null,
    status, players_online: players, players_max: max, cpu_percent: cpu, memory_used_mb: mem,
    uptime_seconds: status === "online" ? up * 60 : null, tick_rate: status === "online" ? 20 : null,
    address: "192.168.1.20:25565", public_address: status === "online" ? "play.example.net:25565" : null, icon_url: null, ...extra,
  });
  const instances = [
    mk("i1", "Survival", "minecraft", "paper", "1.21.4", "online", 8, 20, 14, 2310, 372),
    mk("i2", "Creative", "minecraft", "fabric", "1.21.4", "online", 2, 10, 6, 1480, 95, { ports: { game: 25566 } }),
    mk("i3", "Modded", "minecraft", "neoforge", "1.21.1", "offline", 0, 20, 0, 0, 60 * 30, { ports: { game: 25567 } }),
    mk("i4", "Minigames", "minecraft", "purpur", "1.21.4", "online", 9, 30, 22, 2890, 640, { ports: { game: 25568 } }),
    mk("i5", "Test Server", "minecraft", "vanilla", "1.21.4", "offline", 0, 5, 0, 0, 60 * 72, { ports: { game: 25569 } }),
  ];
  instances.forEach((i) => { i.address = `192.168.1.20:${i.ports.game}`; if (i.public_address) i.public_address = `play.example.net:${i.ports.game}`; });
  const node = { id: "n1", name: "My PC", transport: { kind: "local" }, paired_devices: [{ id: "d1", name: "Phone", paired_at: iso(9000) }], created_at: iso(90000) };
  const health = { cpu_percent: 31, memory_used_mb: 9200, memory_total_mb: 32768, disk_used_gb: 412, disk_total_gb: 1000,
    lan_address: "192.168.1.20", instances_running: 3, uptime_seconds: 86400 * 3 + 7200, os: "Windows 11", cpu_count: 16 };
  const settings = { language: "en", accent: "cyan", density: "comfortable", instancesDir: null, javaPaths: {}, javaPath: null,
    launchAtStartup: false, runInTray: true, allowUpnp: true, listenOnLan: true };
  const line = (m, level, body, speaker) => ({ timestamp: iso(m), level, message: `[Server thread/INFO]: ${body}`, body, speaker: speaker || null });
  const consoleLines = [
    line(30, "info", "Done (6.412s)! For help, type \"help\""),
    line(24, "info", "Gabrieli2806 joined the game", { name: "Gabrieli2806", kind: "join" }),
    line(22, "info", "<Gabrieli2806> hey! anyone up for mining?", { name: "Gabrieli2806", kind: "chat", message: "hey! anyone up for mining?" }),
    line(21, "info", "Troxtrunks joined the game", { name: "Troxtrunks", kind: "join" }),
    line(20, "info", "<Troxtrunks> I'm in, let me grab my gear", { name: "Troxtrunks", kind: "chat", message: "I'm in, let me grab my gear" }),
    line(14, "info", "Saved the game"),
    line(9, "info", "nightmare_33n joined the game", { name: "nightmare_33n", kind: "join" }),
    line(7, "info", "<nightmare_33n> found diamonds in the north cave", { name: "nightmare_33n", kind: "chat", message: "found diamonds in the north cave" }),
    line(3, "info", "Troxtrunks has made the advancement [Diamonds!]"),
    line(1, "info", "<Gabrieli2806> on my way", { name: "Gabrieli2806", kind: "chat", message: "on my way" }),
  ];
  const games = [{ id: "minecraft", name: "Minecraft", accent: "emerald", icon: "minecraft", distributions: [{ id: "vanilla", name: "Vanilla", accent: "#6f8f4e", summary_key: "games.minecraft.vanilla", has_loader_version: false, glyph: "vanilla", addon_loaders: ["vanilla"], addon_dir: "mods", addon_project_type: "mod" }, { id: "paper", name: "Paper", accent: "#3b82f6", summary_key: "games.minecraft.paper", has_loader_version: false, glyph: "paper", addon_loaders: ["paper"], addon_dir: "plugins", addon_project_type: "plugin" }, { id: "fabric", name: "Fabric", accent: "#c9b79c", summary_key: "games.minecraft.fabric", has_loader_version: true, glyph: "fabric", addon_loaders: ["fabric"], addon_dir: "mods", addon_project_type: "mod" }, { id: "neoforge", name: "NeoForge", accent: "#e8903a", summary_key: "games.minecraft.neoforge", has_loader_version: true, glyph: "neoforge", addon_loaders: ["neoforge"], addon_dir: "mods", addon_project_type: "mod" }, { id: "purpur", name: "Purpur", accent: "#a855f7", summary_key: "games.minecraft.purpur", has_loader_version: false, glyph: "purpur", addon_loaders: ["purpur"], addon_dir: "plugins", addon_project_type: "plugin" }],
    runtime: { kind: "java", min_major: 21 }, ports: [{ key: "game", default: 25565, protocol: "tcp", primary: true }], settings: [], agreement: { url: "https://aka.ms/MinecraftEULA" } }];
  const pl = (n) => ({ name: n, uuid: null });
  const players = { allowed: [pl("Gabrieli2806"), pl("Troxtrunks"), pl("nightmare_33n"), pl("Diluza"), pl("Steve"), pl("Alex"), pl("megato13"), pl("helloworld1040")], operators: [{ name: "Gabrieli2806", level: 4 }], banned: [] };
  const fixtures = {
    instances_list: instances, instance_get: (a) => instances.find((i) => i.id === a.instanceId || i.id === a.id) || instances[0],
    node_get: node, node_health: health, settings_get: settings, games_list: games,
    node_requirements: [{ id: "java", status: "ready", found: "Java 21" }, { id: "docker", status: "optional" }, { id: "memory", status: "ready" }, { id: "disk", status: "ready" }],
    instance_console: consoleLines, players_online: ["Gabrieli2806", "Troxtrunks", "nightmare_33n", "Diluza", "Steve", "Alex", "megato13", "helloworld1040"], players_lists: players, players_identities: {},
    backups_list: [], files_list: [], addons_installed: [], addons_list: [], java_overview: [], java_runtimes: [], storage_locations: [],
    instance_disk_usage: { totalBytes: 2400000000, fileCount: 18420, computedAt: new Date().toISOString(), largest: [
      { name: "world", isDir: true, bytes: 1650000000 }, { name: "world_nether", isDir: true, bytes: 310000000 }, { name: "plugins", isDir: true, bytes: 214000000 },
      { name: "paper.jar", isDir: false, bytes: 52000000 }, { name: "logs", isDir: true, bytes: 41000000 }] }, remote_status: { enabled: false }, mcp_status: { enabled: false }, console_completions: [], instance_log_files: [],
  };
  let cb = 1;
  window.__TAURI_INTERNALS__ = {
    metadata: { currentWindow: { label: "main" }, currentWebview: { windowLabel: "main", label: "main" }, windows: [{ label: "main" }], webviews: [{ windowLabel: "main", label: "main" }] },
    transformCallback: () => cb++, unregisterCallback: () => {},
    convertFileSrc: (p) => p,
    invoke: async (cmd, args) => {
      window.__calls = window.__calls || []; window.__calls.push(cmd);
      if (cmd.startsWith("plugin:event|listen")) return cb++;
      if (cmd.startsWith("plugin:")) return /is(Maximized|Fullscreen|Focused|Minimized)|isDecorated/.test(cmd) ? false : null;
      const f = fixtures[cmd];
      if (typeof f === "function") return f(args || {});
      if (f !== undefined) return f;
      return null;
    },
  };
  localStorage.setItem("nebula.firstRunDone", "1");
  localStorage.setItem("nebula.tourDone", "1");
})();
