package net.g2806.nebula.game;

import android.os.Bundle;
import net.g2806.nebula.game.console.TabletopConsole;
import net.kdt.pojavlaunch.Architecture;
import net.kdt.pojavlaunch.MainActivity;
import net.kdt.pojavlaunch.Tools;
import net.kdt.pojavlaunch.lifecycle.ContextExecutor;
import net.kdt.pojavlaunch.prefs.LauncherPreferences;
import net.kdt.pojavlaunch.value.launcherprofiles.LauncherProfiles;
import net.kdt.pojavlaunch.value.launcherprofiles.MinecraftProfile;
import org.json.JSONObject;
import java.io.File;
import java.io.FileOutputStream;
import java.nio.charset.StandardCharsets;

/** Private game process. The request is never written to disk: it contains the
 * short-lived Minecraft token. The launcher/account screens remain in Nebula. */
public final class GameActivity extends MainActivity {
    private static File exitFile;
    private static boolean exitReported;
    private boolean created;
    private TabletopConsole console;
    private SdlGrabSync grabSync;
    JSONObject request;
    String renderer = Graphics.DEFAULT_RENDERER;

    @Override public void onCreate(Bundle state) {
        try {
            String payload = getIntent().getStringExtra("nebulaRequest");
            String launch = getIntent().getStringExtra(BootstrapActivity.LAUNCH);
            getIntent().removeExtra("nebulaRequest");
            // Only a launch that the bootstrap made in this very process may
            // start a JVM. Android recreating this screen after the game's
            // process died (from Recents, "back to the game") brings back the
            // old intent; running it would fail half-way and close. Instead,
            // leave cleanly so Nebula can start a fresh game.
            if (payload == null || launch == null || !launch.equals(BootstrapActivity.currentLaunch)) {
                // Ends the process before the engine's onCreate would run.
                abandon();
                return;
            }
            request = new JSONObject(payload);
            setExitFile(request.getString("gameDir"));
            // A normal quit goes through System.exit: record it as clean unless
            // the engine already reported an error code.
            Runtime.getRuntime().addShutdownHook(new Thread(() -> reportExit(0), "nebula-exit"));
            ContextExecutor.setApplication(getApplication());
            LauncherPreferences.loadPreferences(this);
            JSONObject graphics = request.optJSONObject("graphics");
            // Written before the engine reads them (sustained mode, resolution).
            Graphics.applyPreferences(graphics);
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
            renderer = Graphics.resolveRenderer(this, graphics);
            LauncherProfiles.load();
            MinecraftProfile profile = new MinecraftProfile();
            profile.name = request.getString("name");
            profile.lastVersionId = id;
            profile.gameDir = request.getString("gameDir");
            profile.pojavRendererName = renderer;
            LauncherProfiles.mainProfileJson.profiles.put(id, profile);
            // No account data is ever placed in the engine's profile store.
            LauncherProfiles.write();
            LauncherPreferences.DEFAULT_PREF.edit().putString(LauncherPreferences.PREF_KEY_CURRENT_PROFILE, id)
                .putInt("allocation", request.getInt("memory")).commit();
            LauncherPreferences.PREF_RAM_ALLOCATION = request.getInt("memory");
            Tools.LOCAL_RENDERER = renderer;
            getIntent().putExtra(INTENT_MINECRAFT_VERSION, id);
            super.onCreate(null);
            created = true;
            console = new TabletopConsole(this);
            grabSync = new SdlGrabSync();
        } catch (Throwable error) {
            reportExit(-1, error.getClass().getSimpleName() + ": " + error.getMessage());
            android.util.Log.e("NebulaGame", "Cannot initialize game engine", error);
            // Without the engine's onCreate there is no window to explain in;
            // Nebula shows the reason from the exit marker instead.
            if (!created) { abandon(); return; }
            // The process must not outlive a failed start: a JVM cannot be
            // created twice in one process, and Nebula would refuse the next
            // Play while it lingers.
            new android.app.AlertDialog.Builder(this).setTitle("Minecraft")
                .setMessage(error.getClass().getSimpleName() + ": " + error.getMessage())
                .setPositiveButton(android.R.string.ok, null)
                .setOnDismissListener(dialog -> abandon()).show();
        }
    }

    /** Close the game's window and its process, leaving nothing in Recents. */
    void abandon() {
        finishAndRemoveTask();
        android.os.Process.killProcess(android.os.Process.myPid());
    }

    /** The game's exit code for Nebula's monitor; the first report wins (an
     *  error code from the engine before the clean-exit shutdown hook). */
    public static void reportExit(int code) { reportExit(code, null); }

    /** Also records why, on a second line, for Nebula to show in the instance. */
    static synchronized void reportExit(int code, String reason) {
        if (exitReported) return;
        try {
            if (exitFile != null) {
                write(exitFile, reason == null ? Integer.toString(code) : code + "\n" + reason.replace('\n', ' '));
                exitReported = true;
            }
        } catch (Exception ignored) { /* The Rust monitor treats a missing marker as failure. */ }
    }

    /** Where this launch reports how it ended (inside the instance's folder). */
    static void setExitFile(String gameDir) {
        exitFile = new File(gameDir, ".nebula-game-exit");
    }

    public void showConsoleOptions() { if (console != null) console.showOptions(); }
    public void onConsoleControlsLoaded() { if (console != null) console.controlsLoaded(); }
    public void onConsoleEditorStarting() { if (console != null) console.enterEditor(); }

    @Override public void exitEditor() {
        super.exitEditor();
        if (console != null) console.exitEditor();
    }

    @Override protected void onStart() {
        super.onStart();
        if (console != null) console.start();
        if (grabSync != null) grabSync.start();
    }

    @Override protected void onStop() {
        if (grabSync != null) grabSync.stop();
        if (console != null) console.stop();
        super.onStop();
    }

    @Override protected void onPause() {
        if (console != null) console.cancelInput();
        super.onPause();
    }

    @Override public void onWindowFocusChanged(boolean focused) {
        super.onWindowFocusChanged(focused);
        if (!focused && console != null) console.cancelInput();
    }

    @Override protected void onDestroy() {
        if (console != null) console.close();
        super.onDestroy();
    }

    private static void write(File file, String text) throws Exception {
        try (FileOutputStream output = new FileOutputStream(file)) { output.write(text.getBytes(StandardCharsets.UTF_8)); }
    }

    @Override protected void onSaveInstanceState(Bundle state) {
        // A JVM cannot be recreated from Activity saved state; do not serialize tokens.
    }
}
