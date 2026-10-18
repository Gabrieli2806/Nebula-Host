import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";
import { fileURLToPath, URL } from "node:url";
import { readFileSync } from "node:fs";

const pkg = JSON.parse(readFileSync(new URL("./package.json", import.meta.url), "utf8")) as { version: string };

// Tauri drives the dev server; keep the port fixed and fail loudly if it is taken
// so the shell never silently attaches to some other project's server.
export default defineConfig({
  plugins: [react(), tailwindcss()],
  define: { __APP_VERSION__: JSON.stringify(pkg.version) },
  resolve: {
    alias: { "@": fileURLToPath(new URL("./src", import.meta.url)) },
  },
  clearScreen: false,
  server: {
    host: "127.0.0.1",
    port: 4173,
    strictPort: true,
    watch: { ignored: ["**/src-tauri/**", "**/.legacy/**"] },
  },
  build: {
    target: "chrome120",
    sourcemap: true,
    chunkSizeWarningLimit: 900,
  },
});
