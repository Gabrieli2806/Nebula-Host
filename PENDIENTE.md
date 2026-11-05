# Pendiente

Estado del proyecto tras la auditoría del 05-11-2026 y lo que falta por hacer,
ordenado por prioridad. El detalle de Google Play está en
[docs/google-play-proposal.md](docs/google-play-proposal.md) y el de tiendas y
licencias en [docs/store-compliance-review.md](docs/store-compliance-review.md).

## Hecho en esta auditoría

**Launcher en Android, con paridad con el resto de la app.**
- Una instancia abierta se comporta como un servidor abierto:
  - en el móvil, la barra inferior pasa a ser ← Instancias · Content · Add · Log · Settings;
  - en el escritorio, la barra lateral muestra ← Your instances, Play / Close y los paneles.
- La sección y el panel van en la URL: el botón Atrás de Android vuelve a la biblioteca.
- Las cuatro secciones caben en una fila.
- La cabecera de la instancia ya no esconde el nombre detrás de Play.

**Bugs corregidos.**
- **Play en las tarjetas del Library:** tocarlo abría la instancia en vez de lanzar el juego.
- **Dirección de internet (UPnP) en el Overview:** la IP no se veía y la etiqueta se superponía.
- **Slider de memoria en el móvil:** sus etiquetas se pisaban. En el launcher, además, hablaba de "servidor".
- **Primera ejecución:** la detección mandaba a *welcome* al abrir un atajo a una instancia en una instalación nueva.
- **Ctrl+B en ventana estrecha:** abría una capa vacía, restos de un *drawer* que ya no existe.

**Limpieza.**
- 28 reglas CSS sin uso en `launcher.css`.
- Comentarios "Owner: … agent" en los CSS.
- `work-gradle.log` (116 KB) quitado del repositorio y `*.log` ignorado.
- Dos *warnings* de Rust.

**Documentación.**
- README, `docs/LAUNCHER.md` y `docs/ARCHITECTURE.md` actualizados: describían una versión anterior.
- Nuevo `docs/google-play-proposal.md`.

## Verificado

| Comprobación | Resultado |
|---|---|
| `npm run check` (typecheck + lint) | ✅ 0 errores. 18 *warnings* de fast-refresh, ya existentes (antes 21) |
| `npm run build` | ✅ |
| `cargo test --workspace --no-default-features --locked` | ✅ 258 tests, 2 ignorados |
| `cargo build -p nebula-host --no-default-features --bin nebula-node` | ✅ (el binario que publica el CI) |
| Interfaz con un backend simulado (Playwright, móvil 412 px y escritorio, tema claro y oscuro) | ✅ Capturas revisadas y navegación probada: Play en tarjeta, pestañas, Atrás, enlaces antiguos `?instance=`, instancia inexistente |

**No se pudo verificar aquí** (el contenedor no tiene SDK de Android, ni webkit2gtk, ni MinGW):

- La app de escritorio completa (`tauri build`).
- El APK.
- El código solo-Windows de `launcher/media.rs`. El cambio es mover tres `let` al bloque `#[cfg(windows)]` que los usa.
- **Probar en un teléfono real:** el botón Atrás, la barra inferior dentro de una instancia y Play / Return to game.

## Pendiente

### Alta prioridad

1. **Firma estable del APK.**
   - Problema: el workflow `android.yml` compila `--debug`, y el runner de GitHub genera una clave *debug* nueva en cada ejecución. Cada release sale firmada con una clave distinta, así que Android no puede actualizar una sobre la otra y obliga a desinstalar, perdiendo los datos.
   - Solución: un keystore de release en los *secrets* del repositorio.
2. **Política de privacidad y descargo de Mojang.**
   - Requisitos de Play y de Microsoft Store, sin hacer todavía.
   - En la app hacen falta el enlace en *Ajustes › Acerca de* y el texto "NOT AN OFFICIAL MINECRAFT PRODUCT…".
3. **Archivo LICENSE.**
   - `Cargo.toml` declara MIT, pero no hay archivo.
   - El APK incluye Amethyst (LGPL-3.0) y OpenJDK (GPLv2 + Classpath Exception): hay que incluir sus avisos y ofrecer el código fuente.
4. **Páginas de 16 KB.**
   - A partir del 01-02-2027, Play no acepta actualizaciones que no las soporten.
   - Hay que comprobar todas las `.so`. Las más dudosas: los JRE precompilados y las de Amethyst, que se compilan con el NDK 27.3.
   - Añadir esa verificación al CI.

### Para Google Play

Ver la propuesta. En resumen:

5. Un sabor `play` (feature `play-store`) sin código de terceros: sin mods, loaders, modpacks, plugins ni `.jar` importados. Las reglas se imponen en el backend.
6. Un job de CI que genere el AAB de release.
7. Medir el tamaño. Si pasa del límite, mover los JRE a Play Feature Delivery.
8. Ficha de Play: Data safety, declaración de los dos servicios `specialUse` con vídeo, IARC y pista de prueba cerrada antes de producción.

### Media prioridad

9. **Modrinth `User-Agent`.** Hoy es `NebulaHost/<versión> (G2806)`. Modrinth pide un contacto: añadir un e-mail o una URL (`src-tauri/src/game/mod.rs`).
10. **Errores al pulsar Play desde una tarjeta.** Ahora se muestran bajo los botones. Revisar en un dispositivo que el texto no desborde la tarjeta.
11. **Comentario desactualizado en `NodeTransport::Paired`** (`src-tauri/src/model.rs`). Dice "Not implemented yet", pero los nodos remotos funcionan por el frontend (`src/lib/connection.ts`). Decidir si el modelo del backend se usa o se elimina.
12. **18 *warnings* de fast-refresh.** Mover las funciones y constantes exportadas junto a componentes (`ui/index.tsx`, `ui/StatusChip.tsx`, `i18n/index.tsx`…) a módulos propios. Es cosmético, pero deja el lint limpio.
13. **Android 17 (API 37).** Cuando se suba el `targetSdk`, pedir el permiso `ACCESS_LOCAL_NETWORK` para gestionar el PC por la LAN y marcar como solo lectura las `.so` cargadas con `System.load()`.
14. **Launcher.** Falta el instalador de Quilt, y lanzar el juego en Linux y macOS.

### Decisiones abiertas

Están en `docs/google-play-proposal.md` §8:

- ¿Opción A (solo control remoto) u opción B (control remoto + launcher vanilla) para la primera subida a Play?
- ¿Qué `applicationId` lleva cada sabor (Play y APK `full`)?
- ¿Quién publica en Play: el autor original o este fork? Afecta al nombre del paquete, a la cuenta de desarrollador y al ID de aplicación de Microsoft aprobado por Mojang.
- ¿Hosting de servidores en el teléfono dentro de la versión de Play?
- ¿Qué licencia tiene el proyecto?
