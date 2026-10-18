# Nebula Host: revisión legal y de políticas de tienda

> **Aviso.** Esto es una evaluación de riesgos hecha con fuentes primarias, no asesoría legal. El autor no es abogado. Antes de publicar con fines comerciales, conviene que un abogado de propiedad intelectual revise los puntos marcados como **riesgo medio o alto**.
> Fecha de la revisión: 18-10-2026. Versiones consultadas: Microsoft Store Policies **7.20** (ms.date 14-09-2026), Minecraft EULA y Minecraft Usage Guidelines (minecraft.net, consultadas hoy) y Google Play Policy Center (©2026).

---

## 0. Resumen ejecutivo

| Tema | Veredicto | Riesgo |
|---|---|---|
| Publicar en Microsoft Store (MSIX, Win32 full trust) | **Permitido**, con condiciones (dependencias declaradas, política de privacidad, categoría correcta, descargo de afiliación) | Bajo a medio |
| Google Play: app de **control remoto** de nodos | **Permitido** | Bajo |
| Google Play: **alojar servidores en Android** (descargar `.jar`, JVM, mods) | Zona gris. Choca con "Device and Network Abuse" (código ejecutable descargado) y con los límites de servicios en primer plano | **Alto** |
| Uso del nombre "Minecraft" | Permitido solo como **descripción secundaria** ("para Minecraft: Java Edition") y con descargo | Bajo si se respeta |
| **EULA de Minecraft** | **Hoy se acepta automáticamente en nombre del usuario.** Es un incumplimiento que bloquea la publicación y hay que corregirlo antes | **Alto (bloqueante)** |
| Logos de terceros en etiquetas | Paper, Fabric, NeoForge y Purpur: permitidos con condiciones. Vanilla/Minecraft, Forge, Modrinth, Temurin y mclo.gs: usar glifo propio | Bajo si se sigue la tabla |
| Tráfico remoto sin cifrar (HTTP + token en la LAN) | Choca con la política 10.5.4 de Microsoft Store ("criptografía moderna") | Medio |

---

## 1. Microsoft Store (MSIX)

Fuente: <https://learn.microsoft.com/en-us/windows/apps/publish/store-policies> (versión 7.20).

### 1.1 Descarga de software adicional (server jars, Java de Adoptium, mods y plugins de Modrinth)

- **10.2.2 (código dinámico):** *"Your product must not attempt to fundamentally change or extend its described functionality … through any form of dynamic inclusion of code."* La clave está en "described functionality". Descargar el software de servidor que el usuario elige **es** la función descrita de Nebula Host. No es un cambio de funcionalidad, así que está permitido **si la ficha de la tienda lo explica con claridad**.
- **10.2.3:** *"…must not offer to install secondary software that is not developed by you and does not enhance the functionality of your product."* Java (Temurin) y los jars de servidor son imprescindibles para la función principal ("enhance the functionality"). No se ofrecen como bundleware. Para mantenerse dentro de la norma:
  - nunca ofrecer software ajeno a la función (barras de herramientas, antivirus, etc.);
  - instalar Java en la carpeta de datos de la app (como hoy, con slots propios), sin tocar el `JAVA_HOME` del sistema sin consentimiento (10.2.8).
- **10.2.4 (dependencias):** *"Your product may depend on non-integrated software … if you disclose the dependency at the beginning of the description in metadata."* **Acción:** la primera línea de la descripción debe decir algo como "Requiere conexión a Internet para descargar el software de servidor (Mojang, PaperMC, Fabric, Forge, NeoForge, Purpur) y un runtime de Java (Eclipse Temurin), que la app instala por ti."
- **10.1.5 (complementos):** con consentimiento del usuario y después de la instalación, se permite adquirir *"Add-ons or extensions, excluding non-Microsoft drivers or NT services, that enhance the functionality"*. Los mods y plugins entran aquí. **No instalar servicios NT ni drivers.** El código actual no crea servicios de Windows ni reglas de firewall (verificado con grep: no hay `sc.exe`, `netsh` ni `windows_service`).
- **11.12 (UGC y mods de terceros):** si la app se integra con una plataforma de mods de terceros, debe *"clearly disclose to users when content is not created, published, or sourced by the developer"* e integrarse con sus mecanismos de denuncia cuando existan. **Acción:** en el navegador de addons, mostrar "Contenido de Modrinth, no creado por Nebula Host" y un enlace "Reportar en Modrinth" a la página del proyecto.

### 1.2 Procesos en segundo plano, puertos y UPnP

- La política no prohíbe abrir puertos ni ejecutar procesos hijo. MSIX con `runFullTrust` (ya declarado en `src-tauri/msix/AppxManifest.xml`) no tiene el aislamiento de red de AppContainer.
- **10.6 (capacidades):** las capacidades declaradas deben corresponder a funciones reales. Hoy se declaran `internetClient` y `privateNetworkClientServer`. Como la app recibe conexiones desde Internet vía UPnP, conviene **añadir `internetClientServer`** para que el manifiesto describa con precisión lo que hace, aunque en full trust no es estrictamente necesario.
- **10.2.8:** abrir puertos en el router con UPnP modifica la red, no Windows. Aun así, hay que hacerlo **solo con consentimiento explícito** (interruptor apagado por defecto o confirmación) y explicarlo. La bandeja del sistema debe dejar claro que hay servidores en marcha.
- **10.2.7:** la desinstalación debe ser limpia. Hay que documentar qué pasa con los mundos (se quedan en la carpeta de datos) y ofrecer "eliminar datos".

### 1.3 Categoría y representación (10.1 y 10.1.1)

- 10.1 obliga a categorizar como **juego** los productos de *"remote game play and/or control functionality of games running on other devices"*. Nebula Host no transmite gameplay. Es un gestor de servidores, así que la categoría correcta es **Developer tools** o **Utilities & tools**. Si la certificación lo cuestiona, hay que argumentarlo en las notas de certificación. Ojo: la 10.2.5 exige que los juegos se instalen y actualicen solo por la Store. Por eso **no hay que categorizarlo como juego**.
- 10.1.1: no debe *"mislead customers as to its … relationship to other products"* y el nombre *"must not contain marketing or descriptive text"*. Título: **"Nebula Host"** a secas. Nada de "Nebula Host – Minecraft Server".
- 10.1.3: los términos de búsqueda *"Not use other product titles unless those products are also published by you"*. **No usar "Minecraft" como search term.** Se puede usar en la descripción (ver §3).

### 1.4 Privacidad (10.5)

- **10.5.1:** *"Product types that inherently have access to Personal Information must always have privacy policies. These include … Desktop Bridge and Win32 products."* **La política de privacidad es obligatoria** y su URL se introduce en Partner Center.
- **10.5.2:** publicar datos personales en un servicio externo exige **opt-in** con explicación previa y la posibilidad de revocarlo. El "Share log" a mclo.gs **ya pide confirmación** (`src/features/server/panels/console/ShareLog.tsx`) y advierte que el log puede contener IPs y nombres. Bien. Falta que la opción "no volver a preguntar" (`nebula.mclogsNoConfirm`) se pueda **revertir desde Ajustes** (requisito de "rescind").
- **10.5.3:** los logs contienen nombres e IPs de **otros jugadores** (no clientes de la app). Como la subida es manual, puntual y la decide el administrador, es defendible. Aun así, **conviene redactar las IPs antes de subir** (la API de mclo.gs y su cliente oficial ya lo hacen; confirmar que el texto que envía Nebula pasa por ese filtro o hacerlo en local).
- **10.5.4:** *"If your product collects, stores or transmits Personal Information, it must do so securely, by using modern cryptography."* **Punto débil:** el canal escritorio↔nodo (`src-tauri/src/remote/`) es HTTP plano con token Bearer. Por él viajan consola, nombres de jugadores, IPs y el propio token. **Recomendación:** TLS con certificado autofirmado del nodo, cuya huella se fija (pin) durante el emparejamiento. El endpoint MCP es solo localhost y tiene defensa contra DNS rebinding, así que es aceptable.
- Otros terceros que reciben datos y deben figurar en la política: **crafatar.com / mc-heads.net** (UUID/nombre del jugador para avatares), **api.mojang / launchermeta.mojang.com**, **fill.papermc.io**, **meta.fabricmc.net**, **maven.minecraftforge.net**, **maven.neoforged.net**, **api.purpurmc.org**, **api.modrinth.com**, **api.adoptium.net** y **api.mclo.gs** (logs públicos que expiran a los 90 días según la respuesta de su API; privacidad del operador Aternos GmbH en <https://aternos.gmbh/en/mclogs/privacy>).

### 1.5 Clasificación por edad (11.11)

- Obligatorio el cuestionario IARC. Las respuestas relevantes son: la app **permite interacción entre usuarios** (los jugadores chatean en el servidor; el administrador ve el chat en la consola), **comparte la ubicación**: no, **compras digitales**: no. Probable resultado: PEGI 3/7 o IARC 3+ con el descriptor "Users Interact". Si en el futuro hay chat propio o contenido generado visible para otros, se aplica 11.12 (código de conducta y mecanismo de denuncia).

---

## 2. Google Play (app móvil)

Fuentes: Device and Network Abuse <https://support.google.com/googleplay/android-developer/answer/16559646> y requisitos de foreground services <https://support.google.com/googleplay/android-developer/answer/13392821>.

### 2.1 Opción A: control remoto de nodos (sin ejecutar servidores en el teléfono)

**Permitido y de bajo riesgo.** Solo habla HTTP(S) con nodos de la LAN que el usuario empareja. Requisitos:

- **Formulario de Data safety** y **política de privacidad** (obligatorios en Play). Declarar: identificadores de dispositivo o token de emparejamiento, logs de la app y los datos de jugadores que se muestran.
- **Red local:** Android está introduciendo la protección de red local (opt-in en Android 16, con permiso específico previsto para versiones posteriores). Hay que pedirlo solo al descubrir o emparejar nodos y justificarlo en la UI. *Verificar el estado exacto en la documentación de Android en el momento de implementarlo.*
- **No usar `VpnService`** salvo que la VPN sea la función principal: Play restringe VpnService a apps cuya función núcleo sea una VPN. Si en el futuro se quiere acceso remoto fuera de la LAN, mejor un relay o túnel propio sobre TLS que una VPN en el dispositivo.
- Tráfico cifrado (TLS con pin, ver 1.4). El `cleartextTrafficPermitted` de Android obliga a configurarlo explícitamente si se queda en HTTP.

### 2.2 Opción B: alojar servidores Minecraft en Android

**Riesgo alto. No se recomienda para Play sin una consulta previa a Google.**

- Política literal: *"An app may not download executable code (such as dex, JAR, .so files) from a source other than Google Play."* Excepción: *"This restriction does not apply to code that runs in a virtual machine or an interpreter."*
  - Los `server.jar`, los loaders y los mods son **JAR descargados de fuera de Play**. Se podría argumentar que corren dentro de una JVM (la excepción de VM/intérprete; la redacción completa habla de VMs con acceso *indirecto* a las APIs de Android). Pero el revisor puede no aceptarlo, y los mods pueden cargar código nativo (`.so`) por JNI, lo que sí queda prohibido.
  - El runtime Java (binarios nativos) **tendría que ir empaquetado en el APK/AAB**. No se puede descargar de Adoptium en tiempo de ejecución.
- **Foreground services:** un servidor encendido horas necesita un FGS. Los tipos disponibles no encajan bien: `dataSync` tiene un tope de tiempo en Android 15+, `specialUse` pasa por revisión manual y `connectedDevice` exige un dispositivo externo. Hay que presentar declaración, vídeo y justificación en Play Console. Además, *"must not bypass System power management unless eligible."*
- Precedente práctico: las apps de terceros con "Minecraft" en el nombre reciben con frecuencia rechazos por **propiedad intelectual / suplantación** en Play. Hay que tener lista la respuesta (descargo, sin logos de Mojang, nombre propio).
- **Alternativa recomendada:** publicar en Play solo la **opción A** y, si se quiere hosting en Android, distribuirlo fuera de Play (F-Droid o APK directo), donde estas restricciones no aplican.

---

## 3. Minecraft / Mojang

Fuentes: EULA <https://www.minecraft.net/en-us/eula> y Usage Guidelines <https://www.minecraft.net/en-us/usage-guidelines>.

### 3.1 Uso del nombre "Minecraft"

- Guías, apartado "Naming guidelines": *"You may use the Minecraft name in a secondary name, secondary title, or description if you: Do so because it is necessary to describe your creations or their purpose honestly and fairly; Ensure that the secondary title … is not the dominant element …; Don't use any other aspect of any of our brand or assets as part of any related branding, including as a logo"*. Su propio ejemplo: *"Minecraft – the ultimate help app" (we're not cool with this)*.
- **Permitido:** "Nebula Host: servidores para Minecraft: Java Edition en tu PC" (subtítulo o descripción) y "Vanilla (Minecraft: Java Edition)" en la UI.
- **No permitido:** "Minecraft Host", "Nebula Minecraft", el logo de Minecraft, el bloque de césped ni la tipografía Minecraftia en el icono o la marca.
- **Descargo obligatorio** ("Essential guidelines"): *"Prominently include the disclaimer similar to the following: 'NOT AN OFFICIAL MINECRAFT [PRODUCT/SERVICE]. NOT APPROVED BY OR ASSOCIATED WITH MOJANG OR MICROSOFT' on your product, listing, description, website…"*. Además, hay que dejar claro **quién es el responsable** y **un contacto que no sea chat ni foro** (por ejemplo, un e-mail de soporte).
- EULA, resumen: *"You may develop tools, plug-ins and services as long as they do not seem official or approved by us, such as by using our logos."*
- Caso Microsoft Store: Mojang pertenece a Microsoft. Publicar en la tienda de Microsoft **no** implica permiso. Rige la 11.2 (*"used as permitted by the rights holder"*), y el permiso del titular son justamente estas guías.
- **Texto en la UI que conviene cambiar:** `src/i18n/en.ts:835` dice *"The official server, exactly as Mojang ships it."*. Es correcto como descripción del jar, pero puede leerse como "oficial" dentro de la app. Mejor: "The unmodified Minecraft: Java Edition server, downloaded directly from Mojang."

### 3.2 Software de servidor y redistribución

- EULA: *"For the server version of Minecraft: Java Edition, you can install it on a server and host online play. However, you must not distribute anything we've made unless we specifically agree to it."* y *"we need all game downloads and updates to come from a source that we authorize."*
  - Nebula **descarga el jar vanilla de Mojang** (`launchermeta.mojang.com`) en el equipo del usuario. Correcto.
  - **Riesgo a vigilar:** la función de enviar o transferir servidores a nodos remotos (y la futura "transferir server a otra PC") **copia el jar de Mojang entre máquinas del mismo usuario**. Es razonablemente uso propio, pero lo más seguro es que **cada nodo descargue los binarios de su fuente oficial** y solo se transfieran los mundos y la configuración. Nunca hay que alojar jars de Mojang en servidores propios de Nebula ni en una CDN.
  - Paper y Purpur aplican sus parches en el equipo del usuario (paperclip), así que no redistribuyen el jar de Mojang. Forge y NeoForge usan instaladores que descargan de Mojang. Correcto.
- Guías, "Do not redistribute our games or any alterations of our games or game files". Mismo punto.

### 3.3 Aceptación del EULA: **incumplimiento actual**

Código revisado:

```rust
// src-tauri/src/game/minecraft/launch.rs:114
/// Accept the EULA on the user's behalf once they have said yes in the UI.
pub fn write_eula(root: &Path) -> AppResult<()> {
    fs::write(root.join("eula.txt"), "eula=true\n")?;
```

```rust
// src-tauri/src/game/minecraft/mod.rs:131-133 (durante la instalación)
// Minecraft refuses to start until the EULA is accepted; the UI asks
// before we ever get here.
launch::write_eula(ctx.root)?;
```

- El comentario dice que "la UI pregunta", pero **en el frontend no existe ningún diálogo, casilla ni texto sobre el EULA**. `grep -i eula` sobre `src/` no devuelve nada, y tampoco hay "agree" ni el enlace `aka.ms/MinecraftEULA`. Es decir, **`eula=true` se escribe automáticamente en cada instalación**.
- Además, la herramienta MCP `create_server` (`src-tauri/src/mcp/tools.rs:531`) crea e instala servidores **a petición de un agente IA**, sin ningún humano que acepte.
- Por qué importa: el EULA es un contrato entre **el usuario** y Mojang/Microsoft (*"If you … click to accept this EULA, that means you agree"*). El propio servidor exige el `eula=true` precisamente para que el operador lo acepte. Que un tercero lo acepte de forma silenciosa es exactamente lo que el mecanismo intenta evitar, y un revisor de la Store o Mojang lo verían mal.
- **Corrección necesaria (bloqueante):**
  1. En el asistente de creación (y en importar mundo o servidor), mostrar una casilla **sin marcar por defecto**: "Acepto el [EULA de Minecraft](https://aka.ms/MinecraftEULA)". El enlace debe abrirse en el navegador. Sin la casilla marcada, no se puede crear el servidor.
  2. Guardar la aceptación por servidor (fecha y versión) y escribir `eula.txt` **solo** si existe. Opción alternativa: aceptación global en el primer uso, revocable en Ajustes, con el enlace siempre visible.
  3. **MCP:** `create_server` no debe escribir `eula=true`. Debe dejar el servidor en el estado "pendiente de aceptar EULA" hasta que el usuario lo confirme en la UI, o exigir que el usuario haya activado la aceptación global en la app (nunca un parámetro que el agente pueda poner a `true`).
  4. Nodos remotos: la aceptación se hace en la UI del escritorio y viaja como dato explícito. El nodo headless no la asume nunca.

### 3.4 Monetización

- Las guías consideran "commercial use" cualquier uso compartido, aunque sea gratis. Una app gratuita de gestión cumple. Si se cobra por **Nebula Host** (la herramienta, no el juego), es software propio y no contiene activos de Mojang, así que en principio es admisible siempre que no use su marca como reclamo.
- Si los usuarios cobran por sus servidores, las reglas ("Servers and hosting": mismo precio para todos, nada de pay-to-win, apto para todas las edades) son responsabilidad del **operador**, no de Nebula. Se puede mencionar en los términos de uso.

---

## 4. Nombres y logos de terceros

Principio general: una licencia de **copyright** abierta (MIT o CC-BY) sobre el archivo del logo **no concede derechos de marca**. Aun así, usar un nombre o logo para **identificar con qué es compatible** el software (uso nominativo) es aceptado en la práctica siempre que: (a) el logo no se modifique, (b) sea pequeño y secundario, (c) no se use como marca propia y (d) no sugiera patrocinio. Además, la Store (11.2) exige que el contenido esté *"licensed … or used as permitted by the rights holder"*.

| Proyecto | Fuente | Qué dice | ¿Icono + nombre en etiquetas? |
|---|---|---|---|
| **Minecraft / Vanilla** | EULA y Usage Guidelines | Prohíbe el uso de logos y activos de marca para no parecer oficial: *"Don't use any other aspect of any of our brand or assets … as a logo"*, *"such as by using our logos"* | **No.** Glifo propio neutro (cubo, hoja, etc.) sin parecerse al bloque de césped |
| **Paper** (PaperMC) | <https://docs.papermc.io/misc/assets/> | Permite el logomark *"to represent Paper-Server in downloads, server selectors, and similar places"*. Prohíbe alterar colores, dimensiones o transparencia, derivados, añadir marca propia o venderlo | **Sí**, con el archivo oficial **sin recolorear** (nada de monocromo ni tintado con el tema) |
| **Purpur** | Repo `PurpurMC/PurpurWebsite` (MIT; `src/components/PurpurLogo.astro`) | Logo distribuido bajo MIT; no hay guía de marca publicada | **Sí**, incluyendo el aviso MIT en la pantalla de licencias. Riesgo bajo |
| **Fabric** | Repo `FabricMC/fabricmc.net` (MIT; `assets/logo.png`) | Logo en un repo MIT; no hay guía de marca | **Sí**, con aviso MIT y sin modificar. Riesgo bajo |
| **Forge** (MinecraftForge) | Repo `MinecraftForge/Web` (`images/logo.svg`) **sin licencia declarada**; no hay política de marca pública | Sin licencia rige "todos los derechos reservados" | **No**, glifo propio (o pedir permiso a los mantenedores por escrito). El nombre "Forge" en texto sí se puede usar |
| **NeoForge** | Repo `neoforged/websites` (**CC-BY-4.0**; incluye favicon y `static/img/authors/neoforged.png`) | Se puede reutilizar con atribución | **Sí**, con la atribución "Logo NeoForged, CC BY 4.0" en la pantalla de licencias y sin modificar (o indicando cambios) |
| **Modrinth** | <https://github.com/modrinth/code/blob/main/COPYING.md> | *"The use of Modrinth branding elements, including … the wrench-in-labyrinth logo … is strictly prohibited without explicit written permission from Rinth, Inc."* | **No.** Solo texto "Modrinth" (por ejemplo, "Fuente: Modrinth") |
| **Eclipse Temurin / Adoptium** | <https://adoptium.net/docs/logo-styleguide/> y <https://www.eclipse.org/logotm/> | Marcas de la Eclipse Foundation; la política de marcas restringe usos y no hay permiso explícito para productos de terceros | **No.** Texto "Eclipse Temurin" con ® en la primera mención y "Java is a trademark of Oracle" en licencias |
| **mclo.gs** (Aternos GmbH) | <https://api.mclo.gs> | API pública (60 req/min por IP, logs que expiran a los 90 días); no hay guía de marca | **No.** Solo texto "mclo.gs" y enlace a su política de privacidad |

Recomendaciones de implementación:
- Guardar los SVG/PNG oficiales **sin tocar** en `src/assets/brands/` con un `NOTICE` por archivo (origen, licencia y fecha).
- Usarlos solo a tamaño de etiqueta (14–18 px) junto al nombre. Nunca en el icono de la app, en las capturas principales de la Store ni como elemento de marketing dominante.
- Como el sistema visual es "Nebula clásico", si un logo a color desentona, la alternativa coherente es **glifos propios para todos** (incluidos los permitidos). Es la opción de riesgo cero.
- Mantener una ruta de retirada: si algún proyecto lo pide, el icono se cambia por el glifo propio sin publicar una nueva versión de la UI (un mapa `brandIcon[type] → own | official`).

---

## 5. Lista de acciones para estar listos para tienda

### Bloqueantes
1. **EULA:** casilla de aceptación explícita (desmarcada) con enlace a <https://aka.ms/MinecraftEULA>; escribir `eula.txt` solo tras aceptar; MCP `create_server` sin aceptación implícita (§3.3).
2. **Política de privacidad** publicada (URL en Partner Center y en Play) y enlazada desde Ajustes › Acerca de. Contenido mínimo:
   - Qué datos se manejan: configuración y mundos (en local), nombres, UUID e IPs de jugadores en los logs, token de emparejamiento de nodos y diagnósticos de la app.
   - Que **Nebula no tiene servidores propios ni telemetría** (si es así). Los datos se quedan en el equipo o en los nodos del usuario.
   - Terceros con los que se conecta y qué reciben (lista de §1.4): Mojang, PaperMC, Fabric, Forge, NeoForged, Purpur, Modrinth, Adoptium, crafatar/mc-heads (UUID) y mclo.gs (solo al compartir, con expiración de 90 días).
   - UPnP: qué puertos abre y cómo cerrarlos.
   - MCP: endpoint local, desactivado por defecto (confirmarlo), y qué puede hacer un agente.
   - Derechos del usuario: borrar datos, revocar el "no volver a preguntar" de mclo.gs, revocar nodos emparejados y un contacto por e-mail.
   - Menores: la app no está dirigida a menores de 13 años y no recoge sus datos (Store 10.5.8, COPPA).
3. **Descargo de afiliación** visible en la ficha de la tienda, en el sitio web y en Ajustes › Acerca de:
   > "NOT AN OFFICIAL MINECRAFT PRODUCT. NOT APPROVED BY OR ASSOCIATED WITH MOJANG OR MICROSOFT." / "NO ES UN PRODUCTO OFICIAL DE MINECRAFT. NO ESTÁ APROBADO POR MOJANG NI MICROSOFT NI ASOCIADO CON ELLOS."
   Más: "Minecraft is a trademark of Mojang AB. Paper, Purpur, Fabric, Forge, NeoForge, Modrinth y Eclipse Temurin son marcas de sus respectivos titulares." Más responsable y contacto (e-mail).

### Muy recomendables
4. **Cifrado escritorio↔nodo** (TLS con pin de certificado en el emparejamiento) para cumplir Store 10.5.4 y Play.
5. **Ficha de la Store:**
   - Título "Nebula Host".
   - Subtítulo o primera línea: "Crea y gestiona servidores de Minecraft: Java Edition en tu PC o en otros equipos de tu red."
   - Segunda línea (10.2.4): la dependencia de descargas (servidores y Java).
   - Sin "Minecraft" en los search terms ni logos de Mojang en capturas o icono.
   - Categoría Developer tools o Utilities (no Games).
   - Notas de certificación: explicar que la app descarga software de servidor elegido por el usuario y abre puertos solo con su consentimiento.
6. **Pantalla de licencias** (Ajustes › Acerca de › Licencias de código abierto), generada automáticamente:
   - Rust: `cargo about` o `cargo-license` sobre el `Cargo.lock` (Tauri y plugins son MIT/Apache-2.0; incluir los textos).
   - JS: `license-checker` o `rollup-plugin-license` sobre `package-lock.json` (React MIT, etc.).
   - Fuentes **Quicksand** y **JetBrains Mono** (vía `@fontsource-variable`): **SIL OFL 1.1**. Incluir el texto de la OFL y el copyright. No vender las fuentes por separado.
   - Atribuciones de logos según la tabla del §4 (MIT de Fabric y Purpur, CC-BY-4.0 de NeoForge, condiciones de PaperMC).
   - Avisos de marca: Minecraft/Mojang, Java/Oracle y Eclipse Temurin.
   - El propio proyecto **no tiene archivo LICENSE** en la raíz del repo. Hay que decidir la licencia de Nebula Host (propietaria o de código abierto) antes de publicarlo.
7. **Términos de uso** breves: el usuario es responsable de sus servidores, de cumplir el EULA y las reglas de Mojang si monetiza, y del contenido de los mods que instala.
8. **Modrinth:** indicar "contenido de terceros" y añadir el enlace de denuncia (Store 11.12). Respetar sus términos de API (User-Agent identificable con contacto, como pide su documentación).
9. **mclo.gs:** redactar las IPs antes de enviar y añadir un ajuste para volver a pedir confirmación. Mostrar "Los logs subidos son públicos y caducan a los 90 días".
10. **UPnP** apagado por defecto o confirmado en el primer uso.
11. **IARC:** cuestionario con "Users Interact". Nada de compras.

### Para Google Play (si se hace la app móvil)
12. Publicar en Play solo el **controlador remoto**. Si se hace hosting en Android, distribuirlo fuera de Play o consultar antes con el soporte de políticas de Play.
13. Data safety, política de privacidad, permiso de red local justificado, sin VpnService y con TLS.

### Sobre la futura "transferencia de servidor entre PCs"
14. Transferir **mundos y configuración**. En el destino, volver a descargar los binarios (jar de Mojang, loader y Java) desde sus fuentes oficiales, no copiarlos (EULA §"downloads … from a source that we authorize"). La aceptación del EULA también debe existir en el equipo de destino, ligada al usuario.

---

## 6. Fuentes

- Microsoft Store Policies 7.20: <https://learn.microsoft.com/en-us/windows/apps/publish/store-policies>
- Microsoft, denuncias de PI: <https://www.microsoft.com/legal/intellectualproperty/infringement>
- Minecraft EULA: <https://www.minecraft.net/en-us/eula> · enlace corto oficial <https://aka.ms/MinecraftEULA>
- Minecraft Usage Guidelines: <https://www.minecraft.net/en-us/usage-guidelines>
- Google Play, Device and Network Abuse: <https://support.google.com/googleplay/android-developer/answer/16559646> · <https://support.google.com/googleplay/android-developer/answer/9888379>
- Google Play, foreground services: <https://support.google.com/googleplay/android-developer/answer/13392821>
- PaperMC art assets: <https://docs.papermc.io/misc/assets/>
- Purpur website (MIT): <https://github.com/PurpurMC/PurpurWebsite>
- Fabric website (MIT): <https://github.com/FabricMC/fabricmc.net>
- Forge web assets (sin licencia declarada): <https://github.com/MinecraftForge/Web>
- NeoForged websites (CC-BY-4.0): <https://github.com/neoforged/websites>
- Modrinth COPYING.md: <https://github.com/modrinth/code/blob/main/COPYING.md>
- Adoptium logo styleguide: <https://adoptium.net/docs/logo-styleguide/> · Eclipse Trademark Usage Policy: <https://www.eclipse.org/logotm/> · <https://eclipse.org/legal/trademarks.php>
- mclo.gs API: <https://api.mclo.gs> · privacidad: <https://aternos.gmbh/en/mclogs/privacy>
- Resúmenes secundarios consultados (no normativos): <https://conductatlas.com/platform/minecraft/minecraft-usage-guidelines/no-implied-endorsement-restriction/>, <https://www.diffchecker.com/sa9geIp5>

---

ICON_VERDICT:
- vanilla: own-glyph  (Mojang EULA/Usage Guidelines prohíben usar logos o activos de marca de Minecraft en branding de terceros; el bloque de césped también es activo de marca)
- paper: allowed  (PaperMC permite el logomark en "downloads, server selectors"; sin recolorear ni modificar)
- purpur: allowed  (logo en el repo PurpurWebsite bajo MIT; incluir aviso MIT, sin modificar)
- fabric: allowed  (logo en el repo fabricmc.net bajo MIT; incluir aviso MIT, sin modificar)
- forge: own-glyph  (el repo MinecraftForge/Web no declara licencia y no hay política de marca; todos los derechos reservados salvo permiso escrito)
- neoforge: allowed  (repo neoforged/websites bajo CC-BY-4.0; atribución obligatoria)
- modrinth: own-glyph  (COPYING.md: logo prohibido sin permiso escrito de Rinth, Inc.; usar solo el nombre en texto)
- temurin: own-glyph  (marca de la Eclipse Foundation sin permiso explícito para terceros; usar solo texto)
- mclogs: own-glyph  (sin licencia ni guía de marca; usar solo texto)
EULA_AUTO_ACCEPT: yes
