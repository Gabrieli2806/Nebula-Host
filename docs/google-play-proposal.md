# Nebula Host en Google Play: propuesta

> **Aviso.** Es una propuesta técnica basada en fuentes primarias, no asesoría legal.
> Fecha: 05-11-2026. Complementa [store-compliance-review.md](store-compliance-review.md) (§2 y §5),
> que ya recomendaba publicar en Play solo lo que no descarga código.
> Las páginas de ayuda de Play no se pudieron abrir directamente durante esta revisión.
> Su texto viene de copias literales citadas en issues públicos y de los resultados del buscador.
> Lo que no se pudo confirmar va marcado como **(sin verificar)**.

---

## 0. Resumen

- **El problema no es solo Modrinth.** En Android, Nebula descarga y ejecuta en una JVM varias clases de JAR:
  - el **cliente de Minecraft** y sus librerías Java (de Mojang);
  - los **loaders** (Fabric, Forge, NeoForge);
  - los **mods y modpacks** (de Modrinth);
  - si se aloja un servidor en el teléfono, los **server jars** y **plugins**.

  Leída al pie de la letra, la política de Play prohíbe todo eso, incluido el juego vanilla. Solo la excepción de "código que corre en una máquina virtual" lo deja en zona gris.
- **El juego vanilla no se puede empaquetar.** El EULA de Minecraft prohíbe redistribuir los archivos de Mojang, así que el cliente tiene que bajarse de Mojang en el dispositivo. Un launcher en Play depende siempre, como mínimo, de esa excepción.
- **Propuesta.** Un mismo código con dos *sabores* de compilación:
  - **`play`** (Google Play): control remoto de tus máquinas y un launcher **sin código de terceros**. Juega el Minecraft oficial (descargado de Mojang) con contenido que no es código: resource packs, data packs, mundos, skins. Sin mods, sin loaders, sin modpacks, sin plugins y sin importar `.jar`.
  - **`full`** (GitHub Releases / Obtainium / IzzyOnDroid): todo lo que hay hoy.
- **El corte se impone en el backend (Rust), no solo en la interfaz.** Así ningún camino alternativo puede saltárselo: una petición directa, la herramienta MCP o un archivo arrastrado.
- **Fases.**
  - Primero: requisitos generales de Play (firma, AAB, política de privacidad, 16 KB, servicios en primer plano).
  - Luego: el sabor `play` en una pista de prueba cerrada.
  - Solo si Google lo acepta: plantear mods traídos por el usuario (opción C).

---

## 1. Qué dice la política

**Device and Network Abuse**: <https://support.google.com/googleplay/android-developer/answer/16559646> (la antigua `answer/9888379` redirige ahí).

> "An app distributed via Google Play may not modify, replace, or update itself using any method other than Google Play's update mechanism. Likewise, an app may not download executable code (such as dex, JAR, .so files) from a source other than Google Play. This restriction does not apply to code that runs in a virtual machine or an interpreter where either provides indirect access to Android APIs (such as JavaScript in a webview or browser)."

> "Apps or third-party code (e.g., SDKs) with interpreted languages (JavaScript, Python, Lua, etc.) loaded at run time (e.g., not packaged with the app) must not allow potential violations of Google Play policies."

Cómo se aplica a Nebula:

- **La excepción de VM.** OpenJDK es una máquina virtual, y el código de Minecraft no ve las APIs de Android directamente. Ese es el argumento a favor. Pero la excepción no es un puerto seguro:
  - un JAR puede cargar librerías nativas por JNI, y entonces sí tiene acceso directo;
  - los rechazos llegan redactados como *"contains code to download executable code"*, y la decisión es del revisor.
- **Los JAR de terceros son el caso más difícil de defender.** Mods, loaders y plugins los escribe cualquiera, Google no los revisa, y algunos traen `.so`.

**Precedentes** (resumen; detalle en la investigación de esta revisión):

- **PojavLauncher** estuvo en Play. Lo retiraron en enero de 2025, según su mantenedor por un problema de verificación de la cuenta y no por el código descargado (issue #6559). Volvió en abril de 2025 bajo otra cuenta, y el repositorio se archivó en septiembre de 2025.
- **MojoLauncher** dice estar en Play. Apareció como "temporarily unavailable" hacia mayo de 2026; el motivo está **sin verificar**.
- **Amethyst**, **Zalith** y **FoldCraftLauncher** se distribuyen fuera de Play.
- **RetroArch** es el patrón que sí funciona en Play: su versión de Play **no tiene descargador de cores**. Los cores le llegan "from Google's servers" con Play Feature Delivery, y el build de Play es un *flavor* aparte (`playStoreNormal` / `playStorePlus`).

Conclusión: los launchers Java en Play tienen un historial inestable. La vía probada es separar lo que va a Play y que todo el código venga del propio bundle.

---

## 2. Inventario: qué código ejecutable llega al teléfono hoy

| Qué | De dónde | Tipo | ¿Va en el APK? | Para Play |
|---|---|---|---|---|
| La app (Rust, WebView, Kotlin) | APK | `.so`, dex, JS | Sí | ✅ |
| OpenJDK del juego (21 y 25) | `scripts/android-game-jre.mjs` → assets de `game-runtime` | `.so` + módulos Java | Sí | ✅ (falta comprobar 16 KB, §5.4) |
| OpenJDK del servidor (21 y 25) | `scripts/android-jre.mjs` (Termux) → assets | `.so` | Sí | ✅ (ídem) |
| LWJGL, renderers y librerías de Amethyst | `jniLibs` del submódulo | `.so` | Sí | ✅ (ídem) |
| `libnebula_java.so` (arranca la JVM del servidor) | Compilado en el build (`nebula_java.c`) | ejecutable PIE | Sí | ✅ |
| LWJGL y nativos de Mojang | — | — | En Android **no se descargan** (`launcher/client.rs`, filtro de librerías) | ✅ |
| Cliente de Minecraft + librerías Java | Mojang | JAR | No (el EULA impide empaquetarlo) | ⚠️ Zona gris: excepción de VM, juego oficial |
| Assets del juego (texturas, sonidos) | Mojang | datos | No | ✅ No es código |
| Loaders Fabric / Forge / NeoForge | Sus mavens | JAR | No | ❌ Código de terceros |
| Mods | Modrinth | JAR (a veces con `.so` dentro) | No | ❌ |
| Modpacks `.mrpack` | Modrinth | zip con JARs + overrides | No | ❌ |
| Resource packs, data packs | Modrinth / usuario | datos (JSON, imágenes, `.mcfunction`) | No | ✅ Datos (vigilar que no traigan binarios, §4.3) |
| Shader packs | Modrinth | GLSL | No | ➖ Sin código Java, pero **no sirven sin Iris** (un mod), así que no tiene sentido ofrecerlos en `play` |
| `.jar` importado por el usuario (botón o arrastrar) | Almacenamiento del usuario | JAR | No | ⚠️ No es una "descarga" de la app, pero es código sin revisar |
| Server jar vanilla (alojar en el teléfono) | Mojang | JAR | No | ⚠️ Igual que el cliente |
| Paper, Purpur, servidores Fabric, Forge y NeoForge | Sus APIs | JAR | No | ❌ |
| Plugins y mods de servidor | Modrinth | JAR | No | ❌ |

El proyecto ya resolvió la parte nativa de forma compatible con Play. Las JVM van dentro del paquete y se cargan con `dlopen`, nunca con `exec` desde la carpeta de datos (`runtime/android_java.rs`, `android/launcher/nebula_java.c`). **Lo que queda fuera de Play son los JAR.**

---

## 3. Opciones

| | Qué va a Play | Riesgo de rechazo | Coste | Experiencia en Play |
|---|---|---|---|---|
| **A** | Solo control remoto de máquinas (sin launcher ni hosting en el teléfono) | Bajo | Bajo | Limitada: el launcher y el hosting solo en el APK `full` |
| **B** *(recomendada)* | A + launcher de **Minecraft oficial** (cuenta o demo) con resource packs, data packs, mundos y skins. Hosting en el teléfono solo con el server vanilla, o sin hosting | Medio: depende de la excepción de VM, pero solo para código del titular del juego, bajado de su servidor oficial | Medio | Buena para jugar vanilla. Sin mods |
| **C** | B + mods que el **usuario** aporta desde su almacenamiento (selector del sistema), sin catálogo de código dentro de la app | Medio-alto: un revisor puede verlo como una forma de esquivar la norma | Medio | Mods posibles, sin descubrirlos en la app |
| **D** | Todo como hoy | Alto: código de terceros descargado de fuera de Play; precedentes inestables | Ninguno | Completa, hasta que la retiren |

Descartadas:

- **Play Feature Delivery para los mods.** Feature Delivery solo entrega código que *tú* subes en tu bundle. Subir mods ajenos implicaría redistribuirlos, y muchos no lo permiten por licencia. Además el catálogo cambia a diario.
- **Play Asset Delivery.** Los asset packs no admiten código ("no executable code").

---

## 4. Diseño técnico del sabor `play`

### 4.1 Un interruptor de compilación

- **Rust**: una feature de Cargo `play-store` en `src-tauri/Cargo.toml`, activada con `tauri android build --features play-store`.
- **Android**: una propiedad de Gradle o una variable de entorno (por ejemplo `NEBULA_STORE=play`) que lea `app/build.gradle.kts` para:
  - el `applicationId` (ver §7, pregunta 2);
  - un `BuildConfig.STORE` para el código Kotlin si hiciera falta.
- **Por qué no `productFlavors`**: el plugin de Tauri (`buildSrc/.../RustPlugin.kt`) ya crea una dimensión de *flavors* `abi` (`universal`, `arm64`…). Una segunda dimensión cambia los nombres de las tareas y las rutas de salida que espera `tauri android build`. Antes de elegir esa vía hay que probarla.
- **Interfaz**: el frontend no decide nada por su cuenta. Lee lo que el backend permite (§4.2).

### 4.2 Reglas en el backend (la fuente de verdad)

Con `play-store` activo:

| Punto de entrada | Regla |
|---|---|
| `launcher::create` y `launcher::change_version` (`launcher/mod.rs`) | Solo `Loader::Vanilla` |
| `launcher::content::install` (`content.rs`) | Rechazar `ContentKind::Mod` y `ContentKind::Shader` |
| `launcher::content::install_modpack` | Rechazar siempre |
| `launcher::content::import_file` | Rechazar `Mod`; los zip de resource packs pasan por la inspección de §4.3 |
| `launcher::content::update` | Rechazar `Mod` (no debería haber ninguno) |
| Hosting en el teléfono (`service/instances.rs`, `service/addons.rs`) | Opción B: solo la distribución `vanilla` y sin addons. Opción A: hosting desactivado |
| Arrastrar `.jar` sobre Mods/Plugins (`AddonDrop`) | Mismo rechazo en el backend |

- **Error tipado.** Un código nuevo, por ejemplo `ErrorCode::NotInStoreBuild`, que la interfaz traduzca ("Esta versión de la app no instala mods"). Así se sigue la regla de `ARCHITECTURE.md`: el backend nunca devuelve frases.
- **Capacidades.** Ampliar `launcher_capabilities` con `store: "play" | "full"`, `loaders: [...]` y `content_kinds: [...]`.
- **Frontend.** La interfaz oculta lo que no está disponible:
  - el paso de loader en `CreateInstanceDialog` y el filtro de loader en `Library`;
  - las pestañas Mods y Shaders de `Browse`;
  - la sección Modpacks de `LAUNCHER_SECTIONS`;
  - "Add a .jar" en `Content`;
  - las distribuciones no vanilla en `CreateServerDialog` (Android).

### 4.3 Defensa en profundidad para el contenido que sí se permite

- Antes de instalar un resource pack o un data pack, recorrer el zip y **rechazarlo** si contiene `.jar`, `.class`, `.dex`, `.so`, `.dll`, `.dylib`, `.jnilib` o ejecutables. Es el mismo código para descargas e importaciones.
- Mantener la lista de hosts permitidos de Modrinth (`content.rs`) y la verificación de hash.
- Antes de lanzar el juego en `play`, comprobar que en `mods/` de la instancia no haya nada. Las instancias creadas con un build `full` y restauradas desde una copia de seguridad entrarían por ahí.

### 4.4 Mensaje al usuario

- Un texto neutro donde estarían los mods: "Esta versión no instala mods ni loaders."
- **Sin** flujos para instalar otro APK desde la app. `REQUEST_INSTALL_PACKAGES` está restringido en Play, y un "descarga la versión completa aquí" puede leerse como una forma de esquivar la política.
- Un enlace a la web del proyecto en *Acerca de* es lo habitual y basta.

---

## 5. Requisitos de Play que hay que cumplir de todas formas

### 5.1 Firma, formato y CI

- **Formato y firma.** Play exige un **AAB de release** firmado con una *upload key* y Play App Signing. El CI actual (`.github/workflows/android.yml`) solo compila `--debug --apk`.
- **Hallazgo para el APK `full` también.** El APK de cada release va firmado con la clave *debug* del runner de GitHub, que se genera nueva en cada ejecución porque no hay un keystore fijo ni en caché. Dos releases seguidas quedan firmadas con claves distintas, así que **Android no puede actualizar una sobre la otra** y obliga a desinstalar, perdiendo los datos. **Solución:** un keystore de release en los *secrets* del repositorio, usado en ambos sabores, con claves distintas para Play (upload key) y para el APK `full`.
- **Un job nuevo** `android-play` que genere el AAB con `--features play-store` y lo suba como artefacto. Más adelante, subida a la pista interna con la API de Play Developer.

### 5.2 Nivel de API

- Desde el 31-08-2026, las apps nuevas y sus actualizaciones deben apuntar a **API 36**. `app/build.gradle.kts` ya tiene `targetSdk = 36` ✅.
- Al pasar a **API 37** (Android 17) llegan dos cambios:
  - el permiso de ejecución `ACCESS_LOCAL_NETWORK`, que afecta a gestionar tu PC por la LAN y al servidor en la LAN;
  - las `.so` cargadas con `System.load()` deben estar en solo lectura.

### 5.3 Servicios en primer plano

- Hay dos servicios `specialUse`:
  - `ServerService`, para alojar un servidor (`app/src/main/AndroidManifest.xml`);
  - `GameService`, para el juego en segundo plano (`game-runtime`).
- Los dos ya declaran `PROPERTY_SPECIAL_USE_FGS_SUBTYPE`.
- En Play Console hay que declararlos, con descripción y un vídeo de cada uno. Que `specialUse` exija vídeo además del texto está **sin verificar**; conviene tenerlo listo.
- No usar `dataSync` para el servidor: desde API 35 tiene un tope de 6 horas cada 24 horas.

### 5.4 Páginas de memoria de 16 KB

- Según developer.android.com, desde el **01-02-2027** no se podrán publicar actualizaciones que no soporten 16 KB.
- Hay que comprobar **todas** las `.so`: las de Rust y Tauri, `libnebula_java`, las de Amethyst, que `game-runtime` compila con el NDK **27.3**, y las de los dos OpenJDK precompilados.
- **Riesgo principal:** los JRE de Termux y AngelAuraMC vienen ya compilados. Si no están alineados a 16 KB, hay que recompilarlos o pedir builds alineados.
- **Acción:** un paso en el CI que verifique la alineación (`zipalign -c -P 16 -v 4` sobre el AAB/APK y una revisión ELF de los `LOAD` de cada `.so`) y falle si alguna no cumple.

### 5.5 Tamaño

- **Límites.**
  - El módulo base de un AAB tiene un límite de **200 MB** comprimidos según la ayuda de Play, aunque la documentación de desarrolladores habla de 500 MB **(sin verificar cuál aplica)**.
  - Cada módulo de *feature* tiene el mismo límite, y el total de instalación es de 4 GB.
- **Situación actual.** El paquete lleva cuatro JRE: dos del juego y dos del servidor.
- **Acción.**
  - Medir el AAB.
  - Si supera el límite, mover los JRE a módulos de **Play Feature Delivery** bajo demanda. Esos módulos sí pueden llevar código, y Android localiza sus `.so` con `ClassLoader.findLibrary()`.
  - En `play` con opción B sin hosting, los JRE del servidor desaparecen sin más.

### 5.6 Ficha, privacidad y propiedad intelectual

Pendientes de [store-compliance-review.md](store-compliance-review.md) §5 que siguen abiertos para Play:

- **Política de privacidad.** Publicada y enlazada en Play Console y en *Ajustes › Acerca de*. Hoy la app no tiene enlace.
- **Formulario Data safety.** Datos que hay que declarar:
  - token de emparejamiento;
  - logs;
  - nombres y UUID de jugadores;
  - inicio de sesión de Microsoft: el token se guarda en local y no sale del dispositivo salvo hacia Microsoft, Xbox y Mojang.
- **Nombre "Minecraft".** Solo en descripción o subtítulo ("para Minecraft: Java Edition"), nunca en el título ni en el icono. Además el descargo: *"NOT AN OFFICIAL MINECRAFT PRODUCT. NOT APPROVED BY OR ASSOCIATED WITH MOJANG OR MICROSOFT."*
- **Modrinth.** Si `play` sigue mostrando resource packs de Modrinth, poner el crédito en texto y no usar su logo. Además, su API pide un `User-Agent` con contacto: hoy es `NebulaHost/<versión> (G2806)` (`src-tauri/src/game/mod.rs`); añadir un e-mail o una URL.
- **Licencias.**
  - Amethyst (LGPL-3.0) y OpenJDK (GPLv2 + Classpath Exception) obligan a incluir sus avisos y a **ofrecer el código fuente** de lo que se distribuye.
  - El proyecto en sí **no tiene archivo LICENSE**, aunque `Cargo.toml` diga MIT.
  - Conviene que un abogado revise la LGPL-3.0 en una app de tienda.
- **Cuenta de Microsoft.**
  - El ID de aplicación de `launcher/auth.rs` está aprobado por Mojang para el editor que lo registró.
  - Si quien publica en Play es otra persona u organización, necesita su propio registro y aprobación (ver README).

---

## 6. Distribución fuera de Play (sabor `full`)

- **GitHub Releases + Obtainium.** Ya existe. Hace falta la firma estable (§5.1).
- **IzzyOnDroid.** Acepta los APK que publica el desarrollador.
- **F-Droid (repositorio oficial).** Poco realista: exige compilar todo desde el código fuente, incluidos los JRE y las librerías de Amethyst que hoy se descargan ya compilados.
- **Verificación de desarrollador de Android.**
  - Desde 2026 se aplica en algunos países a las apps instaladas desde tiendas.
  - En **2027** se extiende a todo el mundo, también a las apps instaladas fuera de tiendas en dispositivos certificados. Un APK de un desarrollador no registrado solo se podrá instalar con el "flujo avanzado" (una espera única de 24 h) o por ADB.
  - **Acción para 2027:** registrar el paquete del sabor `full` y su clave de firma en la consola de desarrollador de Android.

---

## 7. Plan por fases

| Fase | Trabajo | Resultado |
|---|---|---|
| **0. Bases** | Keystore de release y firma estable en el CI · job del AAB · verificación de 16 KB en el CI · medir tamaño · política de privacidad y descargo en *Acerca de* · `User-Agent` con contacto · LICENSE del proyecto | El APK `full` se puede actualizar; el AAB se puede subir |
| **1. Sabor `play`** | Feature `play-store` · reglas de §4.2 con tests en Rust (`cargo test --features play-store`) · capacidades y ocultación en la interfaz · inspección de zips (§4.3) · `ErrorCode::NotInStoreBuild` con textos en/es | Un AAB sin código de terceros |
| **2. Pista cerrada** | Ficha, Data safety, declaraciones de servicios en primer plano con vídeo, IARC · subir a prueba cerrada y responder a la revisión | Primer veredicto de Google sin exponer la ficha pública |
| **3. Producción** | Publicar · vigilar avisos de política | En Play |
| **4. (Opcional) Mods del usuario** | Solo si la fase 2 sale limpia y tras consultar al soporte de políticas de Play: importar desde el selector del sistema, sin catálogo ni enlaces de descarga de código | Opción C |

---

## 8. Preguntas abiertas para decidir

1. **¿Opción A o B para la primera subida?** A no depende de ninguna excepción. B ofrece un launcher útil desde el primer día.
2. **¿Qué `applicationId` lleva cada sabor?**
   - Hoy es `net.g2806.nebulahost`.
   - Si Play usa el mismo, quien tenga el APK de GitHub no podrá pasar a Play sin desinstalar, porque las firmas son distintas.
   - Si usa otro, conviven dos apps.
   - Recomendación: Play con el ID principal y `full` con un sufijo (`.full`), avisando a los usuarios actuales.
3. **¿Quién publica en Play: el autor original (Gabrieli2806) o este fork?**
   - Afecta al nombre del paquete.
   - Afecta a la cuenta de desarrollador, que hay que verificar. PojavLauncher perdió su ficha por esto.
   - Afecta al ID de aplicación de Microsoft aprobado por Mojang.
4. **¿Hosting en el teléfono dentro de `play`?** Puede ser solo vanilla o ninguno. Lo segundo reduce tamaño, servicios en primer plano y riesgo.
5. **¿Licencia del proyecto?** `Cargo.toml` dice MIT, pero falta el archivo, y el APK incluye componentes LGPL y GPL.

---

## 9. Fuentes

- Device and Network Abuse: <https://support.google.com/googleplay/android-developer/answer/16559646>.
  - Texto citado, tomado de issues públicos que lo copian literalmente: robolectric#8563 (2023), Mixpeal/flutter_paypal#15 (2023), HMS-Core/hms-flutter-plugin#175 (2022).
- Requisito de nivel de API: <https://developer.android.com/google/play/requirements/target-sdk>
- Páginas de 16 KB: <https://developer.android.com/guide/practices/page-sizes>
- Cambios de comportamiento de Android 10 (W^X): <https://developer.android.com/about/versions/10/behavior-changes-10>. Android 17: <https://developer.android.com/about/versions/17/behavior-changes-17>
- Tipos de servicio en primer plano: <https://developer.android.com/develop/background-work/services/fgs/service-types>. Tiempos límite: <https://developer.android.com/develop/background-work/services/fgs/timeout>
- Play Feature Delivery: <https://developer.android.com/guide/playcore/feature-delivery/on-demand>. Play Asset Delivery: <https://developer.android.com/guide/playcore/asset-delivery>
- Verificación de desarrollador: <https://developer.android.com/developer-verification>
- PojavLauncher: issues #6559, #6572 y #3604 de PojavLauncherTeam/PojavLauncher. MojoLauncher: issue #351 de MojoLauncher/MojoLauncher.
- RetroArch en Play (sin descargador de cores, cores por Feature Delivery): ficha de Play y `build.gradle` de libretro/RetroArch.
- API de Modrinth (`User-Agent`, límite de peticiones): `apps/docs/public/openapi.yaml` en modrinth/code. Marca: `COPYING.md` en modrinth/code.
- Minecraft Usage Guidelines: <https://www.minecraft.net/en-us/usage-guidelines>. EULA: <https://aka.ms/MinecraftEULA>
