package net.g2806.nebula.game;

import net.kdt.pojavlaunch.MainActivity;
import net.kdt.pojavlaunch.Tools;
import net.kdt.pojavlaunch.multirt.MultiRTUtils;
import net.kdt.pojavlaunch.prefs.LauncherPreferences;
import net.kdt.pojavlaunch.utils.JREUtils;
import org.json.JSONArray;
import org.json.JSONObject;
import java.io.File;
import java.util.ArrayList;
import java.util.List;

public final class GameBridge {
    private GameBridge() {}

    /** Replaces the engine's runCraft: Nebula has already resolved the version,
     *  libraries and arguments; this adds the Android runtime pieces. */
    public static void run(MainActivity activity) throws Throwable {
        if (!(activity instanceof GameActivity)) throw new IllegalStateException("Game requests must originate in Nebula");
        GameActivity game = (GameActivity) activity;
        JSONObject request = game.request;
        String lwjgl = "3.3.3";
        JSONArray libraries = request.getJSONObject("metadata").getJSONArray("libraries");
        for (int index = 0; index < libraries.length(); index++) {
            if (libraries.getJSONObject(index).optString("name").startsWith("org.lwjgl:lwjgl:3.4")) lwjgl = "3.4.1";
        }
        Tools.sLwjglVersion = lwjgl;
        Tools.iLwjglVersion = lwjgl.equals("3.4.1") ? 341 : 333;
        Tools.lwjglNativesDir = new File(Tools.DIR_DATA, "lwjgl-" + lwjgl + "-natives/arm64-v8a").getAbsolutePath();
        File modules = new File(Tools.DIR_GAME_HOME, "lwjgl3/" + lwjgl);
        List<String> classpath = new ArrayList<>();
        classpath.add(new File(modules, "lwjgl.jar").getAbsolutePath());
        classpath.add(new File(modules, "lwjgl-" + lwjgl + "-merged-modules.jar").getAbsolutePath());
        File[] jars = modules.listFiles();
        if (jars == null) throw new IllegalStateException("Android LWJGL modules missing");
        for (File jar : jars) {
            if (jar.getName().endsWith(".jar") && !jar.getName().contains("lwjglx") && !classpath.contains(jar.getAbsolutePath())) classpath.add(jar.getAbsolutePath());
        }
        JSONArray original = request.getJSONArray("classpath");
        for (int index = 0; index < original.length(); index++) classpath.add(original.getString(index));
        List<String> args = new ArrayList<>();
        Tools.getCacioJavaArgs(args, false, activity);
        if (hasSdl(libraries)) {
            // SDL loads libvulkan in Android's normal linker namespace. Loading
            // a second Vulkan loader through Turnip's isolated namespace gives
            // RenderPearl a different vkGetInstanceProcAddr and invalid handles.
            // Use the phone's Vulkan driver consistently on the SDL path.
            android.system.Os.unsetenv("POJAV_LOAD_TURNIP");
            android.system.Os.unsetenv("VULKAN_PTR");
            android.system.Os.setenv("SDL_VULKAN_LIBRARY", "/system/lib64/libvulkan.so", true);
            LauncherPreferences.PREF_ZINK_PREFER_SYSTEM_DRIVER = true;
            Tools.useSFPEW = false;
            args.add("-javaagent:" + new File(Tools.DIR_DATA, "nebula-graphics/nebula-graphics.jar").getAbsolutePath());
            System.out.println("[Nebula] SDL graphics: shared OpenGL provider; system Vulkan driver");
        }
        args.add("-Djava.library.path=" + Tools.lwjglNativesDir + ":" + Tools.NATIVE_LIB_DIR);
        args.add("-Dorg.lwjgl.librarypath=" + Tools.lwjglNativesDir);
        JSONArray launch = request.getJSONArray("args");
        String joined = android.text.TextUtils.join(":", classpath);
        for (int index = 0; index < launch.length(); index++) {
            String argument = launch.getString(index);
            if (argument.startsWith("-Djava.library.path=") || argument.startsWith("-Dorg.lwjgl.librarypath=")) continue;
            args.add(argument.replace("__NEBULA_ANDROID_CLASSPATH__", joined));
        }
        LauncherPreferences.PREF_RAM_ALLOCATION = request.getInt("memory");
        LauncherPreferences.PREF_ARC_CAPES = false;
        Tools.LOCAL_RENDERER = game.renderer;
        if (Graphics.DEFAULT_RENDERER.equals(game.renderer)) LauncherPreferences.writeMGRendererSettings();
        System.out.println("[Nebula] Renderer: " + game.renderer + ", resolution " + Math.round(LauncherPreferences.PREF_SCALE_FACTOR * 100) + "%");
        JREUtils.redirectAndPrintJRELog();
        JREUtils.launchJavaVM(activity, MultiRTUtils.forceReread(request.getString("runtime")), new File(request.getString("gameDir")), args, "");
    }

    private static boolean hasSdl(JSONArray libraries) throws Exception {
        for (int i = 0; i < libraries.length(); i++) {
            if (libraries.getJSONObject(i).optString("name").startsWith("org.lwjgl:lwjgl-sdl:")) return true;
        }
        return false;
    }
}
