# Android launcher: SDL graphics and home screen shortcuts

Minecraft versions using `org.lwjgl:lwjgl-sdl` must share graphics providers
between SDL and LWJGL. Minecraft 26.3's RenderPearl checks both `glGetError`
and `vkGetInstanceProcAddr`; mismatches stop backend creation.

On this path Nebula uses the system Vulkan loader instead of the isolated
Turnip loader. `SDL_VULKAN_LIBRARY` points at the same system library, and
the bridge prevents Amethyst from enabling its isolated loader again.
The packaged Java agent resolves SDL's Java OpenGL function lookups through
LWJGL's active provider. It leaves RenderPearl's validation intact.
The patch is installed for existing instances as well as new ones.

MobileGlues still initializes the OpenGL compatibility path. Its initialization
message does not prove that Minecraft selected OpenGL as its final backend.
Minecraft also forces OpenGL after an unsuccessful startup. Nebula does not
erase the game's recovery marker or force Vulkan over that recovery decision.
Native Vulkan still needs validation on the actual phone/driver.

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
