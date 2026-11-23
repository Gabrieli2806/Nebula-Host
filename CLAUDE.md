# Nebula Host

Lo común a todas las apps está en `..\CLAUDE.md`. Aquí solo lo específico de esta.

## Qué hace y en qué estado está

Gestor de servidores de juego en tu propio equipo (hoy Minecraft: Java Edition) con un launcher de Minecraft opcional
y un servidor MCP integrado para que una IA cree, arranque, pruebe y diagnostique servidores e instancias, aquí y en
los equipos emparejados. Versión 1.1.0 (`package.json`, `src-tauri/Cargo.toml` y `src-tauri/tauri.conf.json` van a la
par). Identidad de paquete `44733G2806.NebulaHost`, la reservada en la Store; en local se instala el MSIX firmado con
un certificado de desarrollo.

- Tres builds del mismo backend: app de escritorio (Windows y Linux), nodo sin ventana (`nebula-node`) y app Android.
  El detalle está en `README.md` y `docs/ARCHITECTURE.md`; lo pendiente, en `PENDIENTE.md`.
- El código está en inglés y la interfaz pasa por i18n: todo texto visible va en `src/i18n/en.ts` y `es.ts`.

## Comandos

```powershell
# cargo puede no estar en el PATH
$env:PATH = "$env:USERPROFILE\.cargo\bin;$env:PATH"

npm install                # una vez
npm run tauri:dev          # app de escritorio con recarga en caliente
npm run check              # typecheck + lint del frontend
npm run build              # tsc + vite build (deja dist\, que el crate de escritorio incrusta al compilar)

# Pruebas de Rust: lo mismo que ejecuta .github\workflows\ci.yml
cargo test --workspace --locked
cargo test --locked -p nebula-host --no-default-features --features play-store --lib
cargo build --locked -p nebula-host --no-default-features --bin nebula-node

# Solo el servidor MCP (protocolo sobre HTTP real, herramientas, textos)
cargo test --locked -p nebula-host --lib mcp

# Contrato MCP contra la app abierta, con el interruptor de Ajustes encendido (desde la carpeta de apps)
python tools\mcp-check.py nebula-host

# El mismo comprobador sin abrir la app: sirve el endpoint un minuto en el puerto 47391 con herramientas de mentira
cargo test --locked -p nebula-host --lib serve_for_the_checker -- --ignored
# Con la app abierta el puerto está ocupado: otro puerto y la dirección en vez del nombre
$env:NEBULA_MCP_CHECK_PORT = 47399   # y luego: python tools\mcp-check.py http://127.0.0.1:47399/mcp

# MSIX para instalar en local (compila frontend y binario; no hace falta compilar antes)
powershell -File src-tauri\msix\build-msix.ps1 -Arch x64 -Revision <n> -CertThumbprint <huella>
Add-AppxPackage -Path src-tauri\msix\out\NebulaHost_1.1.0.<n>_x64.msix
```

- `-Revision` es la cuarta cifra de la versión. Subirla en cada reinstalación: Windows rechaza un paquete distinto con
  la misma versión. La Store exige 0, que es el valor por defecto.
- El script solo firma si recibe `-CertThumbprint` (certificado del almacén del usuario) o `-Sign <pfx>`; si la firma
  falla avisa y deja el paquete sin firmar. El sujeto del certificado tiene que ser el `-Publisher` del paquete (por
  defecto, el de la Store) y el certificado tiene que estar en Personas de confianza del equipo.
- Cerrar la app antes de `Add-AppxPackage`.
- `cargo clippy` no está instalado en este equipo y la CI no lo ejecuta.
- La extensión de VS Code (`extensions\vscode`) se compila aparte: `npm run compile` y `npm run package` dentro de esa
  carpeta.

## Dónde tocar

- `src-tauri/src/mcp/mod.rs`: el endpoint MCP (transporte, versiones del protocolo, seguridad) y sus pruebas. No sabe
  nada de servidores: recibe las herramientas por el trait `Host`.
- `src-tauri/src/mcp/tools.rs`: las herramientas de servidores, las instrucciones para el agente, las anotaciones, el
  reenvío a equipos emparejados (`call_routed`) y los textos de error con el siguiente paso.
- `src-tauri/src/mcp/launcher_tools.rs`: las herramientas `launcher_*`. `diagnose.rs`: el diagnóstico de fallos y el
  recorte de textos largos. `activity.rs`: la actividad reciente que enseña Ajustes.
- `src-tauri/src/mcp/console_wait.rs`: esperar una línea de la consola (`wait_for_console` y `send_command` con
  `wait_for`). `profile.rs`: `profile_server` (qué comando se pregunta a cada tipo de servidor y spark).
  `prompts.rs`: los prompts MCP (`test_plugin`, `diagnose_crash`).
- `src-tauri/src/service/clone.rs`: la copia de un servidor (`clone_server`); `service/backups.rs`: lo que usa
  `restore_backup`. El cursor de la consola (`console_cursor`, `console_since`) está en `service/instances.rs`.
- Sin un servidor real solo se prueba la lógica (argumentos, patrones, espera, copia de carpetas). Lo que depende de
  un servidor en marcha (`profile_server`, `restore_backup`, `clone_server` de punta a punta) hay que probarlo con la
  app abierta.
- `src-tauri/src/remote/mod.rs` (`POST /agent`): donde un equipo emparejado ejecuta la herramienta que le llega.
- `src/features/settings/McpSettings.tsx`: la tarjeta de Ajustes (una fila desplegable, plegada por defecto).
- `extensions/vscode`: usa el mismo endpoint y depende del texto de algunas respuestas (`list_servers`,
  `list_machines` y `get_server` en JSON; la línea de `list_versions`; las palabras `CRASHED`, `instance_not_found`,
  `agreement_required`). Si se cambian, cambiar también la extensión.
- Una herramienta nueva: entrada en el catálogo con descripción (qué hace, cuándo usarla, qué devuelve) y, si cambia
  algo, revisar las listas `DESTRUCTIVE`, `IDEMPOTENT_CHANGES` y `OPEN_WORLD`. Las pruebas de `tools.rs` fallan si le
  falta algo del contrato.

## Diferencias con el diseño o stack común

- Tauri v2 con backend en Rust y frontend React 19 + TypeScript (Vite, Tailwind 4), no WinUI. También se compila para
  Linux y Android.
- La interfaz está en inglés y español.
- Un agente conectado aquí puede actuar en equipos emparejados (argumento `machine` de cada herramienta), y hay tres
  permisos en Ajustes: solo ver, iniciar el juego con la cuenta del usuario y permitir agentes de equipos emparejados.
