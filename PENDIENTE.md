# Pendiente

Estado del proyecto tras la auditoría del 05-11-2026 y la segunda tanda del
07-11-2026, y lo que falta por hacer, ordenado por prioridad. El detalle de
Google Play está en [docs/google-play-proposal.md](docs/google-play-proposal.md)
y el de tiendas y licencias en
[docs/store-compliance-review.md](docs/store-compliance-review.md).

## Hecho

### Auditoría (05-11)

**Launcher en Android, con paridad con el resto de la app.**
- Una instancia abierta se comporta como un servidor abierto:
  - en el móvil, la barra inferior pasa a ser ← Instancias · Content · Add · Log · Settings;
  - en el escritorio, la barra lateral muestra ← Your instances, Play / Close y los paneles.
- La sección y el panel van en la URL: el botón Atrás de Android vuelve a la biblioteca.

**Bugs corregidos.**
- Play en las tarjetas del Library abría la instancia en vez de lanzar el juego.
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
| Interfaz con un backend simulado (Playwright, móvil y escritorio) | ✅ Navegación del launcher, diálogos sobre la barra inferior, modo Play (sin loaders, sin Modpacks, sin hosting) |
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
