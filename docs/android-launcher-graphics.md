# Android launcher: renderers, SDL graphics and home screen shortcuts

## Renderer and performance settings

Each instance has Android graphics settings (Instance > Settings > Graphics
and performance), stored in `Profile.graphics` and sent with the launch
request. `net.g2806.nebula.game.Graphics` maps them onto the engine:

| Nebula | Engine renderer | Notes |
|---|---|---|
| Automatic / MobileGlues | `opengles_mobileglues` | Default. OpenGL over GLES 3. |
| Zink | `opengles3_desktopgl_zink_kopper` | OpenGL over Vulkan; needs a Vulkan driver. |
| Krypton | `opengles2` | NG-GL4ES; only when the APK carries `libng_gl4es.so`. |
| LTW | `opengles3_ltw` | 1.17+; only when the APK carries `libltw.so`. |

A choice this device cannot run falls back to MobileGlues and the game log
says so. Resolution scale, sustained performance mode and big-core affinity
are written to the engine's preferences before it starts (absent values keep
the engine's per-device defaults). The app asks the phone what it supports
(`deviceGraphics` in `GamePlugin.kt`: Vulkan version, GLES version, which
renderers are packaged) and disables unavailable renderers in the UI.

Packaged engine components are unpacked only when the APK changes (marker
`.nebula-game-assets` holds the install time), and the Java runtime is
unpacked into a staging folder and moved into place, so an interrupted copy
is never taken for a complete runtime.

## SDL graphics


Minecraft versions using `org.lwjgl:lwjgl-sdl` must share graphics providers
between SDL and LWJGL. Minecraft 26.3's RenderPearl checks both `glGetError`
and `vkGetInstanceProcAddr`; mismatches stop backend creation.

On this path Nebula uses the system Vulkan loader instead of the isolated
Turnip loader. `SDL_VULKAN_LIBRARY` points at the same system library, and
the bridge (`net.g2806.nebula.game.GameBridge`) prevents the engine from
enabling its isolated loader again.
The packaged Java agent resolves SDL's Java OpenGL function lookups through
LWJGL's active provider. It leaves RenderPearl's validation intact.
The patch is installed for existing instances as well as new ones.

MobileGlues still initializes the OpenGL compatibility path. Its initialization
message does not prove that Minecraft selected OpenGL as its final backend.
Minecraft also forces OpenGL after an unsuccessful startup. Nebula does not
erase the game's recovery marker or force Vulkan over that recovery decision.
Native Vulkan and the renderer choices still need validation on real phones/drivers.

Run the binding regression check from `src-tauri/gen/android` with JDK 21:

```powershell
.\gradlew.bat :game-runtime:testGraphicsAgent
```

The check transforms the exact SDL class bundled with the APK and verifies
that other SDL methods retain their instructions and resolved symbols. It
does not exercise an Android GPU.

In Library, select the instance and account/demo mode, then choose **Add to
home screen**. Android's launcher asks for confirmation (Android 8+).
The shortcut starts that instance or resumes it when already running.
Only the instance, mode and account ID are saved; authentication tokens remain
in the existing session storage. A fresh process without an authenticated
session requires sign-in in the launcher. Authentication failures are shown
in the selected instance; they never silently switch to demo mode.
