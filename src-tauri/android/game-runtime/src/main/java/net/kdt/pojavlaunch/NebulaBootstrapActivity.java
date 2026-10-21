package net.kdt.pojavlaunch;

import android.app.Activity;
import android.os.Bundle;
import android.content.Intent;
import android.widget.LinearLayout;
import android.widget.ProgressBar;
import android.widget.TextView;
import net.kdt.pojavlaunch.lifecycle.ContextExecutor;
import net.kdt.pojavlaunch.prefs.LauncherPreferences;
import net.kdt.pojavlaunch.utils.LocaleUtils;
import org.json.JSONObject;

/** Extract packaged components off the main thread, before creating a Surface. */
public final class NebulaBootstrapActivity extends Activity {
    @Override public void onCreate(Bundle state) {
        super.onCreate(state);
        String payload = getIntent().getStringExtra("nebulaRequest");
        getIntent().removeExtra("nebulaRequest");
        if (payload == null) { finish(); return; }
        LinearLayout layout = new LinearLayout(this);
        layout.setOrientation(LinearLayout.VERTICAL);
        layout.setGravity(android.view.Gravity.CENTER);
        layout.addView(new ProgressBar(this));
        TextView label = new TextView(this);
        label.setText("Minecraft · Preparing game engine…");
        label.setPadding(24, 32, 24, 24);
        layout.addView(label);
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
            new android.app.AlertDialog.Builder(this).setTitle("Minecraft")
                .setMessage("Cannot start the game engine: " + error.getClass().getSimpleName() + ": " + error.getMessage())
                .setPositiveButton(android.R.string.ok, (dialog, which) -> android.os.Process.killProcess(android.os.Process.myPid())).show();
            return;
        }
        new Thread(() -> {
            try {
                JSONObject request = new JSONObject(payload);
                new NebulaGameAssets(this).prepare(request);
                runOnUiThread(() -> {
                    startActivity(new Intent(this, NebulaGameActivity.class).putExtra("nebulaRequest", request.toString()));
                    finish();
                });
            } catch (Throwable error) {
                android.util.Log.e("NebulaGame", "Game component preparation failed", error);
                runOnUiThread(() -> new android.app.AlertDialog.Builder(this).setTitle("Minecraft")
                    .setMessage("Cannot prepare the game engine: " + error.getClass().getSimpleName())
                    .setPositiveButton(android.R.string.ok, (dialog, which) -> android.os.Process.killProcess(android.os.Process.myPid())).show());
            }
        }, "nebula-game-assets").start();
    }
    @Override protected void onSaveInstanceState(Bundle state) { }
}
