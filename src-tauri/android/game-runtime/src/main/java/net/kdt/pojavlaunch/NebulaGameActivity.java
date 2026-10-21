package net.kdt.pojavlaunch;

import android.os.Bundle;
import android.system.Os;
import net.kdt.pojavlaunch.lifecycle.ContextExecutor;
import net.kdt.pojavlaunch.prefs.LauncherPreferences;
import net.kdt.pojavlaunch.value.launcherprofiles.LauncherProfiles;
import net.kdt.pojavlaunch.value.launcherprofiles.MinecraftProfile;
import org.json.JSONObject;
import java.io.File;
import java.io.FileOutputStream;
import java.io.InputStream;
import java.nio.charset.StandardCharsets;

/** Private game process. The request is never written to disk: it contains the
 * short-lived Minecraft token. The launcher/account screens remain in Nebula. */
public final class NebulaGameActivity extends MainActivity {
    private static File exitFile;
    JSONObject request;

    @Override public void onCreate(Bundle state) {
        try {
            String payload = getIntent().getStringExtra("nebulaRequest");
            getIntent().removeExtra("nebulaRequest");
            if (payload == null) { finish(); return; }
            request = new JSONObject(payload);
            exitFile = new File(request.getString("gameDir"), ".nebula-game-exit");
            ContextExecutor.setApplication(getApplication());
            LauncherPreferences.loadPreferences(this);
            Tools.DEVICE_ARCHITECTURE = Architecture.getDeviceArchitecture();
            Tools.DIR_GAME_NEW = request.getString("root");
            Tools.DIR_HOME_LIBRARY = new File(Tools.DIR_GAME_NEW, "libraries").getAbsolutePath();
            Tools.DIR_HOME_VERSION = new File(Tools.DIR_GAME_NEW, "versions").getAbsolutePath();
            Tools.ASSETS_PATH = new File(Tools.DIR_GAME_NEW, "assets").getAbsolutePath();
            Tools.GAME_PROFILES_FILE = new File(Tools.DIR_GAME_NEW, "launcher_profiles.json").getAbsolutePath();
            String id = request.getString("id");
            File version = new File(Tools.DIR_HOME_VERSION, id + "/" + id + ".json");
            version.getParentFile().mkdirs();
            write(version, request.getJSONObject("metadata").toString());
            LauncherProfiles.load();
            MinecraftProfile profile = new MinecraftProfile();
            profile.name = request.getString("name");
            profile.lastVersionId = id;
            profile.gameDir = request.getString("gameDir");
            profile.pojavRendererName = "opengles_mobileglues";
            LauncherProfiles.mainProfileJson.profiles.put(id, profile);
            // No account data is ever placed in the engine's profile store.
            LauncherProfiles.write();
            LauncherPreferences.DEFAULT_PREF.edit().putString(LauncherPreferences.PREF_KEY_CURRENT_PROFILE, id)
                .putInt("allocation", request.getInt("memory")).commit();
            LauncherPreferences.PREF_RAM_ALLOCATION = request.getInt("memory");
            Tools.LOCAL_RENDERER = "opengles_mobileglues";
            getIntent().putExtra(INTENT_MINECRAFT_VERSION, id);
            super.onCreate(null);
        } catch (Throwable error) {
            reportExit(-1);
            android.util.Log.e("NebulaGame", "Cannot initialize game engine", error);
            new android.app.AlertDialog.Builder(this).setTitle("Minecraft")
                .setMessage(error.getClass().getSimpleName() + ": " + error.getMessage())
                .setPositiveButton(android.R.string.ok, (dialog, which) -> finish()).show();
        }
    }

    public static void reportExit(int code) {
        try { if (exitFile != null) write(exitFile, Integer.toString(code)); }
        catch (Exception ignored) { /* The Rust monitor treats a missing marker as failure. */ }
    }

    private static void write(File file, String text) throws Exception {
        try (FileOutputStream output = new FileOutputStream(file)) { output.write(text.getBytes(StandardCharsets.UTF_8)); }
    }

    @Override protected void onSaveInstanceState(Bundle state) {
        // A JVM cannot be recreated from Activity saved state; do not serialize tokens.
    }
}
