package net.g2806.nebula.game;

import android.os.Handler;
import android.os.Looper;
import java.lang.reflect.Method;
import net.kdt.pojavlaunch.MinecraftGLSurface;
import org.libsdl.app.NebulaSdlMouse;
import org.lwjgl.glfw.CallbackBridge;

/** Versions that use SDL (26.3 and newer) capture the mouse with SDL's relative
 *  mode, and the engine only learns about a capture from GLFW. Without this it
 *  believes the game is always in a menu: the console's stick stays off and the
 *  camera gets absolute positions. Mirror SDL's state into the engine's. */
final class SdlGrabSync implements Runnable {
    private static final long INTERVAL_MS = 100;
    private final Handler handler = new Handler(Looper.getMainLooper());
    private final Method notify;

    SdlGrabSync() throws ReflectiveOperationException {
        notify = CallbackBridge.class.getDeclaredMethod("onGrabStateChanged", boolean.class);
        notify.setAccessible(true);
    }

    void start() { handler.removeCallbacks(this); handler.post(this); }
    void stop() { handler.removeCallbacks(this); }

    @Override public void run() {
        if (MinecraftGLSurface.sdlEnabled) {
            boolean relative = NebulaSdlMouse.relative();
            if (relative != CallbackBridge.isGrabbing()) {
                try {
                    notify.invoke(null, relative);
                } catch (ReflectiveOperationException error) {
                    android.util.Log.e("NebulaGame", "Cannot update the grab state", error);
                    return;
                }
            }
        }
        handler.postDelayed(this, INTERVAL_MS);
    }
}
