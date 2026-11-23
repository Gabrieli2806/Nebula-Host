# Pendiente

Estado del proyecto tras la auditoría del 05-11-2026, la segunda tanda del
07-11-2026 y la fusión con `main` del repo original del 10-11-2026, y lo que falta por hacer, ordenado por prioridad. El detalle de
Google Play está en [docs/google-play-proposal.md](docs/google-play-proposal.md)
y el de tiendas y licencias en
[docs/store-compliance-review.md](docs/store-compliance-review.md).

## Hecho

### Auditoría (05-11)

**Launcher en Android.**
- Al fusionar con `main` del repo original (10-11), su rediseño del launcher (PR #31) pasó a ser la base de la maquetación:
  - lista e instancia lado a lado en pantallas anchas;
  - Play fijo abajo en el móvil;
  - pestañas en una fila.
- La barra de instancia propia que tenía esta rama en el *shell* se retiró, para no duplicar la lista ni el botón Play.
- **Lo que esta rama añade encima:** la sección, la instancia abierta y su panel van en la URL (`?tab=`, `?instance=`, `?panel=`). El botón Atrás de Android vuelve panel → instancia → biblioteca en vez de salir del launcher.

**Bugs corregidos.**
- La IP de internet (UPnP) no se veía en el Overview.
- Las etiquetas del slider de memoria se pisaban en el móvil.
- La primera ejecución mandaba a *welcome* al abrir un atajo a una instancia.
- Ctrl+B en ventana estrecha abría una capa vacía.

**Limpieza y documentación.**
- CSS muerto, un log de Gradle subido por error y *warnings* de Rust.
- README y docs actualizados.
- Nueva propuesta para Google Play.

### Segunda tanda (07-11)

**Sabor Google Play** (feature de Cargo `play-store`, apagada por defecto, `src-tauri/src/flavor.rs`). Las reglas se imponen en el backend, así que valen igual para la interfaz, las herramientas MCP y los archivos arrastrados:
- solo el juego oficial: sin Fabric, Forge, NeoForge ni Quilt;
- solo resource packs: sin mods, modpacks ni shaders;
- rechaza cualquier archivo que lleve código (`.jar`, `.class`, `.dex`, `.so`, ejecutables);
- no aloja servidores en el teléfono. Gestionar un PC sigue igual.

La interfaz oculta lo que el build rechazaría (`src/lib/deviceBuild.ts`). Hay tests de ambos sabores, y el CI los ejecuta.

**Bug de diálogos.** Los diálogos abiertos desde cualquier pantalla quedaban debajo de la barra inferior del móvil, y en el escritorio su fondo no cubría toda la ventana. La causa: la animación de entrada dejaba un `transform` aplicado.

**Fusión con `main` del original (10-11).**
- Los 8 conflictos del launcher se resolvieron partiendo de la versión del original.
- El sabor Play también cubre "compartir mods" entre servidor e instancia (`mod_share.rs`), que es nuevo en `main`.
- Se arregló un error de lint de `main` (`TrayPanel.tsx`) que hacía fallar `npm run check`.

**CI de Android** (`.github/workflows/android.yml`):
- Firma estable si existen los *secrets* `ANDROID_KEYSTORE_BASE64`, `ANDROID_KEYSTORE_PASSWORD`, `ANDROID_KEY_ALIAS` y `ANDROID_KEY_PASSWORD`. Sin ellos funciona como antes y avisa.
- Informe de alineación a 16 KB (`scripts/check-16k.sh`).
- Ejecución manual con `store: play` para compilar el sabor de Play.

**Licencias y privacidad.**
- *Acerca de › Licencias* ahora lista los componentes del APK: Amethyst (LGPL-3.0), OpenJDK (GPLv2 + Classpath Exception), LWJGL y skinview3d.
- Borrador de política de privacidad en `docs/privacy-policy.md`.

## Verificado

| Comprobación | Resultado |
|---|---|
| `npm run check` y `npm run build` | ✅ 0 errores (18 *warnings* de fast-refresh, cosméticos) |
| `cargo test --workspace --no-default-features --locked` | ✅ |
| `cargo test -p nebula-host --no-default-features --features play-store --lib` | ✅ |
| `cargo build -p nebula-host --no-default-features --bin nebula-node` | ✅ |
| Interfaz con un backend simulado (Playwright, móvil y escritorio), sobre el resultado de la fusión | ✅ Sección, instancia y panel en la URL; Atrás recorre panel → instancia → biblioteca; diálogos sobre la barra inferior; modo Play (sin loaders, sin Modpacks, sin Play en instancias con loader, sin hosting) |
| `scripts/check-16k.sh` con librerías de prueba de 4 KB y 16 KB | ✅ |

**No se pudo verificar aquí** (el contenedor no tiene SDK de Android, ni webkit2gtk, ni MinGW):

- **Escritorio y Windows:** la app de escritorio completa y el código solo-Windows de `launcher/media.rs`.
- **APK (CI):** que el APK se genere con la firma nueva y con `--features play-store`. Comprobarlo con una ejecución manual del workflow *Android*.
- **Teléfono real:** el botón Atrás, la barra inferior dentro de una instancia, Play / Return to game y los diálogos.

## Pendiente

### Lo tienes que hacer tú (no se puede desde el código)

1. **Crear el keystore de release y guardarlo en los *secrets* del repositorio.** Mientras no exista, cada APK publicado sigue firmado con una clave distinta y no se puede actualizar sin desinstalar.

   ```bash
   keytool -genkeypair -v -keystore nebula-release.jks -alias nebula -keyalg RSA -keysize 4096 -validity 10000
   ```

   ```bash
   base64 -w0 nebula-release.jks
   ```

   - Pega la salida del segundo comando en `ANDROID_KEYSTORE_BASE64`.
   - Pon las contraseñas en `ANDROID_KEYSTORE_PASSWORD` y `ANDROID_KEY_PASSWORD`, y `nebula` en `ANDROID_KEY_ALIAS`.
   - **Guarda una copia del `.jks` fuera de GitHub**: si se pierde, no se puede volver a publicar una actualización con esa firma.
2. **Licencia del proyecto.** `Cargo.toml` dice MIT, pero no hay archivo LICENSE.
3. **Política de privacidad.** Completa `docs/privacy-policy.md` con el editor y un e-mail de contacto. Después publícala en una URL pública y enlázala en Play Console y en *Acerca de*.
4. **Decisiones de Google Play** (`docs/google-play-proposal.md` §8):
   - ¿Opción A o B?
   - ¿Qué `applicationId` lleva cada sabor?
   - ¿Quién publica: el autor original o este fork? Afecta al nombre del paquete, a la cuenta de desarrollador y al ID de aplicación de Microsoft aprobado por Mojang.
   - ¿Hosting de servidores en el teléfono? Hoy el sabor Play no lo tiene.

### Siguiente trabajo en el código

5. **AAB de release para Play.**
   - Un job que ejecute `tauri android build --aab --features play-store` en modo release, con la firma de subida.
   - Hay que probar antes que el build de release, que activa R8 (`isMinifyEnabled = true`), no rompe las clases del motor del juego que se usan por JNI y reflexión. Si las rompe, hacen falta reglas en `proguard-rules.pro`.
6. **Páginas de 16 KB.**
   - Ejecutar el workflow y leer el informe.
   - Lo más probable es que haya que recompilar o sustituir los JRE precompilados y las librerías de Amethyst, que se compilan con el NDK 27.3.
   - Cuando todo esté alineado, activar `--strict` en el CI.
   - Play exige soportar 16 KB en las actualizaciones a partir del 01-02-2027.
7. **Tamaño del AAB.**
   - Medirlo.
   - En el sabor Play sobran los JRE del servidor, que también ejecutan los instaladores de Forge y NeoForge: ese sabor no tiene ni hosting ni loaders. Excluirlos de ese build reduce bastante el tamaño.
8. **Ficha de Play.** Data safety, declaración de los dos servicios `specialUse` con vídeo, cuestionario IARC y pista de prueba cerrada antes de producción.

### Media y baja prioridad

9. **Modrinth `User-Agent`.** Hoy es `NebulaHost/<versión> (G2806)`. Modrinth pide un contacto: añadir un e-mail o una URL cuando se decida quién publica (`src-tauri/src/game/mod.rs`).
10. **Android 17 (API 37).** Al subir el `targetSdk`:
    - pedir el permiso `ACCESS_LOCAL_NETWORK`;
    - marcar como solo lectura las `.so` cargadas con `System.load()`.
11. **Launcher.** Falta el instalador de Quilt, y lanzar el juego en Linux y macOS.
12. **18 *warnings* de fast-refresh (cosmético).**
    - Arreglarlos de verdad implica mover hooks como `useI18n`, que se importa en cientos de sitios.
    - Alternativa barata: permitir esos nombres con `allowExportNames` en `eslint.config.js`.
13. **Bot de pruebas para agentes (MCP).** Hoy un agente puede instalar un plugin o un mod, arrancar el servidor y leer la consola, pero no comprobar nada que necesite un jugador dentro (un comando de jugador, un evento al entrar, un permiso, un bloque colocado). Falta un jugador de mentira que el agente maneje. No está empezado.
    - **Qué haría.** Entrar al servidor de pruebas con un nombre fijo, ejecutar comandos como jugador, escribir en el chat, moverse o usar un objeto de forma básica, y devolver lo que ese jugador ve (chat, mensajes del sistema, expulsiones).
    - **Dos caminos.**
      - *Cliente sin ventana* (al estilo de mineflayer): un proceso aparte que habla el protocolo del juego. Sirve para cualquier servidor (vanilla, Paper, Fabric, Forge) y prueba el camino real de un jugador, incluido el inicio de sesión. A cambio hay que empaquetar y mantener un cliente del protocolo por versión de Minecraft (mineflayer es Node.js; habría que incluir Node o escribirlo en Rust), y los servidores con mods que exigen sus mods en el cliente (Forge, NeoForge) lo rechazan.
      - *Jugador falso en el servidor* (un plugin o mod propio, como hace Carpet con `/player`): no hay protocolo que mantener y funciona también con mods, pero hay que publicar y mantener un plugin (Bukkit) y un mod (Fabric, NeoForge) por versión, se instala en el servidor que se prueba y no ejercita el inicio de sesión ni los paquetes reales.
      - Propuesta: empezar por el cliente sin ventana para Paper, Purpur, Spigot, vanilla y Fabric sin mods de cliente, que es donde se desarrollan la mayoría de plugins, y dejar el jugador falso para Forge y NeoForge.
    - **Requisito.** El bot no tiene cuenta de Minecraft: el servidor tiene que estar con `online-mode=false`. Solo debe poder entrar en servidores así, y la herramienta debe negarse en los demás en vez de cambiar el ajuste por su cuenta. Nunca usa la cuenta del usuario.
    - **Herramientas que añadiría.** `bot_join` (servidor, nombre; devuelve cuando ha entrado o por qué no), `bot_command` (ejecutar un comando como jugador y devolver la respuesta), `bot_chat` (escribir en el chat), `bot_chat_log` (lo que el bot ha visto, con cursor como `wait_for_console`), `bot_leave`. Quedan ocultas con «solo ver» y se reenvían a equipos emparejados como las demás.
    - **Por decidir.** Cuántos bots a la vez, si puede moverse y romper o colocar bloques, y cómo se limpia si el agente se desconecta (salir solo tras un tiempo sin llamadas).
