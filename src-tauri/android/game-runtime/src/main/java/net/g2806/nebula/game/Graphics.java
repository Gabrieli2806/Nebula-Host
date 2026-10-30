package net.g2806.nebula.game;

import android.content.Context;
import net.kdt.pojavlaunch.Tools;
import net.kdt.pojavlaunch.prefs.LauncherPreferences;
import org.json.JSONObject;

/** The instance's graphics choices from Nebula, mapped onto the engine's
 *  renderers and preferences. Unknown or unsupported choices fall back to
 *  MobileGlues, which works on every OpenGL ES 3 device. */
final class Graphics {
    static final String DEFAULT_RENDERER = "opengles_mobileglues";

    private Graphics() {}

    /** Nebula's renderer id → the engine's. "auto" and anything unknown pick the default. */
    static String engineRenderer(JSONObject graphics) {
        switch (graphics == null ? "auto" : graphics.optString("renderer", "auto")) {
            case "krypton": return "opengles2";
            case "zink": return "opengles3_desktopgl_zink_kopper";
            case "ltw": return "opengles3_ltw";
            default: return DEFAULT_RENDERER;
        }
    }

    /** The renderer to run with: the chosen one when this device can, else the default. */
    static String resolveRenderer(Context context, JSONObject graphics) {
        String wanted = engineRenderer(graphics);
        if (wanted.equals(DEFAULT_RENDERER)) return wanted;
        try {
            // Kopper Zink translates OpenGL to Vulkan: it needs a Vulkan driver,
            // which the engine's own list does not check for this renderer.
            boolean vulkan = !wanted.contains("zink") || Tools.checkVulkanSupport(context.getPackageManager());
            if (vulkan && Tools.checkRendererCompatible(context, wanted)) return wanted;
        } catch (Throwable error) {
            android.util.Log.w("NebulaGame", "Renderer check failed", error);
        }
        System.out.println("[Nebula] Renderer " + wanted + " is not available on this device; using MobileGlues");
        return DEFAULT_RENDERER;
    }

    /** Store the per-instance performance choices where the engine reads them.
     *  Absent values keep the engine's own device-based defaults. */
    static void applyPreferences(JSONObject graphics) {
        if (graphics == null) return;
        android.content.SharedPreferences.Editor edit = LauncherPreferences.DEFAULT_PREF.edit();
        int resolution = graphics.optInt("resolution", 0);
        if (resolution >= 25 && resolution <= 100) edit.putInt("resolutionRatio", resolution);
        else edit.remove("resolutionRatio");
        if (graphics.has("sustained") && !graphics.isNull("sustained")) edit.putBoolean("sustainedPerformance", graphics.optBoolean("sustained"));
        else edit.remove("sustainedPerformance");
        edit.putBoolean("bigCoreAffinity", graphics.optBoolean("bigCores", false));
        edit.commit();
    }
}
