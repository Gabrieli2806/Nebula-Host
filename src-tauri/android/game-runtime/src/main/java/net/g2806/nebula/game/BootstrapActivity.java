package net.g2806.nebula.game;

import android.app.Activity;
import android.os.Bundle;
import android.content.Intent;
import android.widget.LinearLayout;
import android.widget.ProgressBar;
import android.widget.TextView;
import net.kdt.pojavlaunch.Architecture;
import net.kdt.pojavlaunch.R;
import net.kdt.pojavlaunch.Tools;
import net.kdt.pojavlaunch.lifecycle.ContextExecutor;
import net.kdt.pojavlaunch.prefs.LauncherPreferences;
import net.kdt.pojavlaunch.utils.LocaleUtils;
import org.json.JSONObject;

/** Extract packaged components off the main thread, before creating a Surface. */
public final class BootstrapActivity extends Activity {
    static final String LAUNCH = "nebulaLaunch";
    /** The launch this process made; a recreated game screen without it leaves. */
    static volatile String currentLaunch;
    private boolean failed;

    @Override public void onCreate(Bundle state) {
        super.onCreate(state);
        String payload = getIntent().getStringExtra("nebulaRequest");
        getIntent().removeExtra("nebulaRequest");
        // A saved state means Android is recreating this screen after the
        // process died: the request is stale (and its token may be expired).
        if (payload == null || state != null) { finishAndRemoveTask(); return; }
        try {
            GameActivity.setExitFile(new JSONObject(payload).getString("gameDir"));
        } catch (Exception malformed) {
            finishAndRemoveTask();
            return;
        }
        // Nebula's dark surface and accent, so going from the app to the game
        // does not flash an unrelated screen.
        LinearLayout layout = new LinearLayout(this);
        layout.setOrientation(LinearLayout.VERTICAL);
        layout.setGravity(android.view.Gravity.CENTER);
        layout.setBackgroundColor(NebulaStyle.SURFACE);
        ProgressBar progress = new ProgressBar(this);
        progress.setIndeterminateTintList(android.content.res.ColorStateList.valueOf(NebulaStyle.ACCENT));
        layout.addView(progress);
        TextView label = new TextView(this);
        label.setText(getString(R.string.nebula_preparing_game));
        label.setTextColor(NebulaStyle.TEXT_MUTED);
        label.setTextSize(15);
        label.setGravity(android.view.Gravity.CENTER);
        int pad = Math.round(24 * getResources().getDisplayMetrics().density);
        label.setPadding(pad, pad, pad, pad);
        layout.addView(label);
        getWindow().setStatusBarColor(NebulaStyle.SURFACE);
        getWindow().setNavigationBarColor(NebulaStyle.SURFACE);
        setContentView(layout);
        // The engine's classes must be first used on the main thread: loading its
        // native library runs code that needs a Looper (Choreographer), and on a
        // worker thread that aborts the whole process with no message, so the
        // screen just closes. Only the file copying below is slow enough to need
        // a worker thread.
        try {
            ContextExecutor.setApplication(getApplication());
            LocaleUtils.setLocale(this);
            LauncherPreferences.loadPreferences(this);
            Tools.DEVICE_ARCHITECTURE = Architecture.getDeviceArchitecture();
        } catch (Throwable error) {
            android.util.Log.e("NebulaGame", "Game engine initialization failed", error);
            fail("Cannot start the game engine: " + error.getClass().getSimpleName() + ": " + error.getMessage());
            return;
        }
        new Thread(() -> {
            try {
                JSONObject request = new JSONObject(payload);
                new GameAssets(this).prepare(request);
                String launch = java.util.UUID.randomUUID().toString();
                currentLaunch = launch;
                runOnUiThread(() -> {
                    startActivity(new Intent(this, GameActivity.class).putExtra("nebulaRequest", request.toString()).putExtra(LAUNCH, launch));
                    finish();
                });
            } catch (Throwable error) {
                android.util.Log.e("NebulaGame", "Game component preparation failed", error);
                runOnUiThread(() -> fail("Cannot prepare the game engine: " + error.getClass().getSimpleName() + ": " + error.getMessage()));
            }
        }, "nebula-game-assets").start();
    }
    /** Show why the start failed, then end the game's process however the
     *  dialog goes away (OK, back, or leaving the screen), so the next Play
     *  starts clean instead of finding a half-started process. */
    private void fail(String message) {
        failed = true;
        GameActivity.reportExit(-1, message);
        if (isFinishing() || isDestroyed()) { end(); return; }
        new android.app.AlertDialog.Builder(this).setTitle("Minecraft")
            .setMessage(message)
            .setPositiveButton(android.R.string.ok, null)
            .setOnDismissListener(dialog -> end()).show();
    }

    private void end() {
        finishAndRemoveTask();
        android.os.Process.killProcess(android.os.Process.myPid());
    }

    @Override protected void onStop() {
        super.onStop();
        if (failed) end();
    }

    @Override protected void onSaveInstanceState(Bundle state) {
        // Never the request (it holds the token); only that this is a recreation.
        state.putBoolean("nebulaRecreated", true);
    }
}
