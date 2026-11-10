package net.g2806.nebula.game;

import android.content.Context;
import net.kdt.pojavlaunch.Tools;
import net.kdt.pojavlaunch.multirt.MultiRTUtils;
import net.kdt.pojavlaunch.prefs.LauncherPreferences;
import org.json.JSONObject;
import java.io.File;
import java.io.FileOutputStream;
import java.io.InputStream;
/** Unpacks the engine's components and the bundled Java runtime from the APK. */
final class GameAssets {
    private final Context context;
    GameAssets(Context context) { this.context = context; }
    void prepare(JSONObject request) throws Exception {
        // Walking every packaged component is the slow part of starting a game,
        // and they only change when the APK does. Skip it while the marker
        // matches this install of the app.
        File marker = new File(Tools.DIR_DATA, ".nebula-game-assets");
        String build = Long.toString(context.getPackageManager().getPackageInfo(context.getPackageName(), 0).lastUpdateTime);
        if (!build.equals(read(marker))) {
            // Updated independently of the engine: replace its bytes on APK
            // updates, including already-created instances.
            copyAssets("components/nebula-graphics", new File(Tools.DIR_DATA, "nebula-graphics"), true);
            for (String component : new String[]{"caciocavallo", "caciocavallo17", "lwjgl3/3.3.3", "lwjgl3/3.4.1"}) {
                copyAssets("components/" + component, new File(Tools.DIR_GAME_HOME, component));
            }
            for (String component : new String[]{"security", "MioLibPatcher"}) {
                copyAssets("components/" + component, new File(Tools.DIR_DATA, component));
            }
            for (String version : new String[]{"3.3.3", "3.4.1"}) {
                String folder = "lwjgl-" + version + "-natives/arm64-v8a";
                copyAssets("components/" + folder, new File(Tools.DIR_DATA, folder));
            }
            new File(Tools.CTRLMAP_PATH).mkdirs();
            Tools.copyAssetFile(context, "default.json", Tools.CTRLMAP_PATH, false);
            Tools.copyAssetFile(context, "resolv.conf", Tools.DIR_DATA, false);
            try (FileOutputStream output = new FileOutputStream(marker)) { output.write(build.getBytes("UTF-8")); }
        }
        useNebulaControls();
        int major = request.getJSONObject("metadata").getJSONObject("javaVersion").getInt("majorVersion");
        File runtime = new File(Tools.MULTIRT_HOME, "Nebula-" + major);
        if (!new File(runtime, "release").isFile()) {
            // Unpack beside it and move into place, so an interrupted copy is
            // resumed next time rather than taken for a complete runtime.
            File staging = new File(Tools.MULTIRT_HOME, "Nebula-" + major + ".part");
            copyAssets("game-jre/" + major, staging);
            if (runtime.exists()) org.apache.commons.io.FileUtils.deleteDirectory(runtime);
            if (!staging.renameTo(runtime)) throw new java.io.IOException("Cannot finish Java runtime extraction");
        }
        // The runtime ships without libawt_xawt.so; AWT initialisation (used by the
        // game's agents) fails without a stand-in, so install it from the app's
        // native libraries. Done every time: cheap, and it repairs older copies.
        MultiRTUtils.postPrepare("Nebula-" + major);
        request.put("runtime", "Nebula-" + major);
    }

    /** Install the Nebula-styled on-screen controls and make them the default,
     *  unless the person already chose or edited a layout of their own. */
    private void useNebulaControls() throws Exception {
        File nebula = new File(Tools.CTRLMAP_PATH, "nebula.json");
        if (!nebula.isFile()) Tools.copyAssetFile(context, "nebula-controls.json", Tools.CTRLMAP_PATH, "nebula.json", false);
        android.content.SharedPreferences prefs = LauncherPreferences.DEFAULT_PREF;
        String chosen = prefs.getString("defaultCtrl", null);
        android.content.SharedPreferences.Editor edit = prefs.edit();
        // The stock layout counts as "not chosen" only while it is unedited:
        // someone who saved their own buttons over it keeps them.
        File stockFile = new File(Tools.CTRLDEF_FILE);
        boolean stock = (chosen == null || chosen.equals(Tools.CTRLDEF_FILE))
            && (!stockFile.isFile() || sameAsAsset(stockFile, "default.json"));
        if (stock) edit.putString("defaultCtrl", nebula.getAbsolutePath());
        // Labels as the app writes them ("Jump"), not shouted ("JUMP").
        if (!prefs.contains("buttonAllCaps")) edit.putBoolean("buttonAllCaps", false);
        edit.apply();
    }

    private boolean sameAsAsset(File file, String asset) {
        try (InputStream input = context.getAssets().open(asset)) {
            long size = 0;
            byte[] bytes = new byte[8192];
            int count;
            while ((count = input.read(bytes)) != -1) size += count;
            return file.length() == size;
        } catch (Exception unreadable) {
            return false;
        }
    }

    private static String read(File file) {
        try (InputStream input = new java.io.FileInputStream(file)) {
            byte[] bytes = new byte[64];
            int count = input.read(bytes);
            return count > 0 ? new String(bytes, 0, count, "UTF-8") : "";
        } catch (Exception missing) {
            return "";
        }
    }

    private void copyAssets(String asset, File output) throws Exception {
        copyAssets(asset, output, false);
    }

    private void copyAssets(String asset, File output, boolean replace) throws Exception {
        String[] children = context.getAssets().list(asset);
        if (children != null && children.length > 0) {
            if (!output.isDirectory() && !output.mkdirs()) throw new java.io.IOException("Cannot create game assets");
            for (String child : children) copyAssets(asset + "/" + child, new File(output, child), replace);
        } else {
            if (output.isFile() && !replace) return;
            File parent = output.getParentFile();
            if (parent != null) parent.mkdirs();
            File staging = new File(output.getPath() + ".part");
            try (InputStream input = context.getAssets().open(asset); FileOutputStream destination = new FileOutputStream(staging)) {
                byte[] bytes = new byte[65536]; int count;
                while ((count = input.read(bytes)) != -1) destination.write(bytes, 0, count);
            }
            if (!staging.renameTo(output)) throw new java.io.IOException("Cannot finish game asset extraction");
        }
    }

}
