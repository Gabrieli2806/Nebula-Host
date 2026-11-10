package net.g2806.nebula.game.console;

import android.graphics.Rect;
import android.view.MotionEvent;
import android.view.View;
import android.widget.FrameLayout;
import android.widget.Toast;
import androidx.appcompat.app.AlertDialog;
import androidx.core.content.ContextCompat;
import androidx.core.util.Consumer;
import androidx.core.view.ViewCompat;
import androidx.core.view.WindowInsetsCompat;
import androidx.window.java.layout.WindowInfoTrackerCallbackAdapter;
import androidx.window.layout.DisplayFeature;
import androidx.window.layout.FoldingFeature;
import androidx.window.layout.WindowInfoTracker;
import androidx.window.layout.WindowLayoutInfo;
import net.kdt.pojavlaunch.GrabListener;
import net.kdt.pojavlaunch.MinecraftGLSurface;
import net.g2806.nebula.game.GameActivity;
import net.kdt.pojavlaunch.R;
import net.kdt.pojavlaunch.Tools;
import net.kdt.pojavlaunch.customcontrols.ControlLayout;
import net.kdt.pojavlaunch.customcontrols.mouse.Touchpad;
import net.kdt.pojavlaunch.prefs.LauncherPreferences;
import net.kdt.pojavlaunch.utils.TouchControllerInputView;
import org.lwjgl.glfw.CallbackBridge;
import java.util.IdentityHashMap;
import java.util.Map;
import net.kdt.pojavlaunch.customcontrols.buttons.ControlInterface;

/** Observes the actual window posture; changing mode never reparents the surface. */
public final class TabletopConsole implements GrabListener {
    private static final String PREFERENCE = "nebula.console.mode";
    private static final int AUTO = 0, ON = 1, OFF = 2;
    private final GameActivity activity;
    private final FrameLayout root;
    private final ControlLayout game;
    private final MinecraftGLSurface surface;
    private final Touchpad cursor;
    private final ConsolePad pad;
    private final FrameLayout.LayoutParams originalGameParams;
    private final WindowInfoTrackerCallbackAdapter tracker;
    private final Map<View, Integer> fixedControls = new IdentityHashMap<>();
    private final Consumer<WindowLayoutInfo> postureListener = this::postureChanged;
    private final View.OnLayoutChangeListener layoutListener = (v, l, t, r, b, ol, ot, or, ob) -> apply();
    private final View.OnLayoutChangeListener viewportListener = (v, l, t, r, b, ol, ot, or, ob) -> {
        if (r - l != or - ol || b - t != ob - ot) v.post(this::refreshViewport);
    };
    private FoldingFeature fold;
    private boolean active, started, closed, controlsWereVisible, cursorWasEnabled;
    private int mode;

    public TabletopConsole(GameActivity activity) {
        this.activity = activity;
        root = activity.findViewById(R.id.content_frame);
        game = activity.findViewById(R.id.main_control_layout);
        surface = activity.findViewById(R.id.main_game_render_view);
        cursor = activity.findViewById(R.id.main_touchpad);
        originalGameParams = new FrameLayout.LayoutParams((FrameLayout.LayoutParams)game.getLayoutParams());
        mode = LauncherPreferences.DEFAULT_PREF.getInt(PREFERENCE, AUTO);
        if (mode < AUTO || mode > OFF) mode = AUTO;
        pad = new ConsolePad(activity, cursor, activity::onClickedMenu);
        pad.setVisibility(View.GONE);
        pad.setElevation(dp(12));
        root.addView(pad, new FrameLayout.LayoutParams(-1, 1));
        ViewCompat.setOnApplyWindowInsetsListener(pad, (view, insets) -> {
            androidx.core.graphics.Insets safe = insets.getInsets(WindowInsetsCompat.Type.systemGestures() | WindowInsetsCompat.Type.displayCutout() | WindowInsetsCompat.Type.navigationBars());
            pad.safeInsets(safe.left, safe.right, safe.bottom);
            root.post(this::apply);
            return insets;
        });
        ViewCompat.requestApplyInsets(pad);
        root.addOnLayoutChangeListener(layoutListener);
        game.addOnLayoutChangeListener(viewportListener);
        tracker = new WindowInfoTrackerCallbackAdapter(WindowInfoTracker.getOrCreate(activity));
        CallbackBridge.addGrabListener(this);
        root.post(this::apply);
    }

    public void start() {
        if (started || closed) return;
        started = true;
        tracker.addWindowLayoutInfoListener(activity, ContextCompat.getMainExecutor(activity), postureListener);
        root.post(this::apply);
    }

    public void stop() {
        if (!started) return;
        started = false;
        tracker.removeWindowLayoutInfoListener(postureListener);
        cancelInput();
    }

    public void close() {
        stop();
        closed = true;
        root.removeOnLayoutChangeListener(layoutListener);
        game.removeOnLayoutChangeListener(viewportListener);
        CallbackBridge.removeGrabListener(this);
    }

    public void showOptions() {
        cancelInput();
        // The stock controls editor owns the game area until it is closed.
        if (game.getModifiable()) return;
        String[] choices = {activity.getString(R.string.nebula_console_auto), activity.getString(R.string.nebula_console_on), activity.getString(R.string.nebula_console_off)};
        new AlertDialog.Builder(activity)
            .setTitle(R.string.nebula_console_mode)
            .setSingleChoiceItems(choices, mode, (dialog, selected) -> {
                mode = selected;
                LauncherPreferences.DEFAULT_PREF.edit().putInt(PREFERENCE, mode).apply();
                dialog.dismiss();
                apply();
                if (mode == ON && !active) Toast.makeText(activity, R.string.nebula_console_small, Toast.LENGTH_LONG).show();
            })
            .setNeutralButton(R.string.nebula_console_help_action, (dialog, which) -> new AlertDialog.Builder(activity)
                .setMessage(R.string.nebula_console_help).setPositiveButton(android.R.string.ok, null).show())
            .setNegativeButton(android.R.string.cancel, null).show();
    }

    private void postureChanged(WindowLayoutInfo info) {
        fold = null;
        for (DisplayFeature feature : info.getDisplayFeatures()) {
            if (feature instanceof FoldingFeature) { fold = (FoldingFeature)feature; break; }
        }
        apply();
    }

    private void apply() {
        if (closed || root.getWidth() == 0 || root.getHeight() == 0) return;
        boolean horizontalHalf = fold != null && fold.getOrientation() == FoldingFeature.Orientation.HORIZONTAL && fold.getState() == FoldingFeature.State.HALF_OPENED;
        boolean verticalSeparating = fold != null && fold.getOrientation() == FoldingFeature.Orientation.VERTICAL && fold.isSeparating();
        Rect bounds = fold == null ? new Rect() : fold.getBounds();
        int[] location = new int[2];
        root.getLocationInWindow(location);
        ConsoleGeometry.Split split = mode == OFF || game.getModifiable() ? null : ConsoleGeometry.resolve(
            root.getWidth(), root.getHeight(), dp(600), dp(140), pad.minimumPanelHeight(), dp(8),
            mode == ON, horizontalHalf, verticalSeparating, bounds.top - location[1], bounds.bottom - location[1]);
        if (split == null) {
            if (active) leave();
            return;
        }
        FrameLayout.LayoutParams gameParams = (FrameLayout.LayoutParams)game.getLayoutParams();
        FrameLayout.LayoutParams padParams = (FrameLayout.LayoutParams)pad.getLayoutParams();
        int controlsHeight = root.getHeight() - split.controlsTop;
        if (active && gameParams.height == split.gameHeight && padParams.topMargin == split.controlsTop && padParams.height == controlsHeight) {
            hideGameControls();
            return;
        }
        cancelGameTouches();
        pad.cancelInput();
        if (!active) {
            controlsWereVisible = game.areControlVisible();
            cursorWasEnabled = cursor.getDisplayState();
        }
        active = true;
        gameParams.height = split.gameHeight;
        game.setLayoutParams(gameParams);
        hideGameControls();
        padParams.height = controlsHeight;
        padParams.topMargin = split.controlsTop;
        pad.setLayoutParams(padParams);
        pad.setVisibility(View.VISIBLE);
        cursor.enable(true);
        cursor.onGrabState(CallbackBridge.isGrabbing());
        pad.grabChanged();
        root.post(this::refreshViewport);
    }

    private void leave() {
        cancelInput();
        active = false;
        pad.setVisibility(View.GONE);
        game.setLayoutParams(new FrameLayout.LayoutParams(originalGameParams));
        for (Map.Entry<View, Integer> entry : fixedControls.entrySet()) entry.getKey().setVisibility(entry.getValue());
        fixedControls.clear();
        game.setControlVisible(controlsWereVisible);
        if (!cursorWasEnabled) cursor.disable();
        root.post(this::refreshViewport);
    }

    private void refreshViewport() {
        if (closed) return;
        Tools.updateWindowSize(activity);
        surface.refreshSize();
        TouchControllerInputView touch = activity.findViewById(R.id.touch_controller_input);
        touch.setSize(surface.getWidth(), surface.getHeight());
        game.refreshControlButtonPositions();
    }

    public void cancelInput() {
        if (!active) return;
        cancelGameTouches();
        pad.cancelInput();
    }

    private void cancelGameTouches() {
        long time = android.os.SystemClock.uptimeMillis();
        MotionEvent cancel = MotionEvent.obtain(time, time, MotionEvent.ACTION_CANCEL, 0, 0, 0);
        game.dispatchTouchEvent(cancel);
        cancel.recycle();
    }

    public void controlsLoaded() {
        if (active) {
            controlsWereVisible = game.areControlVisible();
            hideGameControls();
        }
        apply();
    }

    public void enterEditor() { if (active) leave(); }
    public void exitEditor() { root.post(this::apply); }

    @Override public void onGrabState(boolean grabbing) {
        root.post(() -> {
            if (closed || !active) return;
            hideGameControls();
            pad.grabChanged();
        });
    }

    private void hideGameControls() {
        game.setControlVisible(false);
        for (ControlInterface control : game.getButtonChildren()) {
            if (!control.getProperties().isHideable) {
                View view = control.getControlView();
                if (!fixedControls.containsKey(view)) fixedControls.put(view, view.getVisibility());
                view.setVisibility(View.GONE);
            }
        }
    }

    private int dp(float value) { return Math.round(value * activity.getResources().getDisplayMetrics().density); }
}
