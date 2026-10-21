package net.kdt.pojavlaunch;
import android.content.Context;
import org.json.JSONObject;
import java.io.File;
import java.io.FileOutputStream;
import java.io.InputStream;
final class NebulaGameAssets {
    private final Context context;
    NebulaGameAssets(Context context) { this.context = context; }
    void prepare(JSONObject request) throws Exception {
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
        File marker = new File(Tools.DIR_DATA, ".nebula-game-assets-330c6eae");
        marker.createNewFile();
        int major = request.getJSONObject("metadata").getJSONObject("javaVersion").getInt("majorVersion");
        File runtime = new File(Tools.MULTIRT_HOME, "Nebula-" + major);
        if (!new File(runtime, "release").isFile()) copyAssets("game-jre/" + major, runtime);
        request.put("runtime", "Nebula-" + major);
    }

    private void copyAssets(String asset, File output) throws Exception {
        String[] children = context.getAssets().list(asset);
        if (children != null && children.length > 0) {
            if (!output.isDirectory() && !output.mkdirs()) throw new java.io.IOException("Cannot create game assets");
            for (String child : children) copyAssets(asset + "/" + child, new File(output, child));
        } else {
            if (output.isFile()) return;
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
