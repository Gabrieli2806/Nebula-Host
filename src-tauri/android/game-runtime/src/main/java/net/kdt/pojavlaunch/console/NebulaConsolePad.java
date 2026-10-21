package net.kdt.pojavlaunch.console;

import android.annotation.SuppressLint;
import android.content.Context;
import android.graphics.Canvas;
import android.graphics.Paint;
import android.graphics.Rect;
import android.graphics.Typeface;
import android.graphics.drawable.GradientDrawable;
import android.graphics.drawable.StateListDrawable;
import android.view.Gravity;
import android.view.MotionEvent;
import android.view.View;
import android.view.ViewConfiguration;
import android.view.ViewGroup;
import android.widget.FrameLayout;
import android.widget.LinearLayout;
import android.widget.TextView;
import androidx.appcompat.widget.AppCompatButton;
import net.kdt.pojavlaunch.MainActivity;
import net.kdt.pojavlaunch.R;
import net.kdt.pojavlaunch.customcontrols.mouse.Touchpad;
import net.kdt.pojavlaunch.prefs.LauncherPreferences;
import org.lwjgl.glfw.CallbackBridge;
import net.kdt.pojavlaunch.LwjglGlfwKeycode;
import java.util.HashMap;
import java.util.ArrayList;
import java.util.Map;

/** Separate touch controls: the original game surface remains in its parent. */
public final class NebulaConsolePad extends LinearLayout {
    /** Keeps view/input tests independent of the native JVM and GPU. */
    public interface Game extends ConsoleInput.Sink {
        boolean grabbing();
        void move(float dx, float dy);
        float cursorX();
        float cursorY();
        int width();
        int height();
        void keyboard();
    }
    private static final int INK = 0xffe3eaf1, MUTED = 0xff99a9b8, ACCENT = 0xff72d6bd;
    private final ConsoleInput input;
    private final Game game;
    private final Stick stick;
    private final LookPad look;
    private final Map<Integer, Finger> fingers = new HashMap<>();

    public NebulaConsolePad(Context context, Touchpad cursor, Runnable settings) {
        this(context, new NativeGame(cursor), settings);
    }

    public NebulaConsolePad(Context context, Game game, Runnable settings) {
        super(context);
        this.game = game;
        this.input = new ConsoleInput(game);
        setOrientation(VERTICAL);
        setBackgroundColor(0xff101820);
        setPadding(dp(12), dp(4), dp(12), dp(8));
        setMotionEventSplittingEnabled(true);
        setFocusable(false);

        LinearLayout toolbar = row();
        TextView title = new TextView(context);
        title.setText(R.string.nebula_console_mode);
        title.setTextColor(ACCENT);
        title.setTextSize(13);
        title.setTypeface(Typeface.DEFAULT, Typeface.BOLD);
        title.setGravity(Gravity.CENTER_VERTICAL);
        toolbar.addView(title, new LayoutParams(0, -1, 1));
        toolbar.addView(action("Esc", () -> { input.key(LwjglGlfwKeycode.GLFW_KEY_ESCAPE, true); input.key(LwjglGlfwKeycode.GLFW_KEY_ESCAPE, false); }), new LayoutParams(dp(64), -1));
        toolbar.addView(action(context.getString(R.string.nebula_console_keyboard), game::keyboard), new LayoutParams(dp(96), -1));
        AppCompatButton options = action("\u22ef", settings);
        options.setContentDescription(context.getString(R.string.nebula_console_menu));
        toolbar.addView(options, new LayoutParams(dp(52), -1));
        addView(toolbar, new LayoutParams(-1, dp(48)));

        LinearLayout body = row();
        FrameLayout movement = new FrameLayout(context);
        stick = new Stick(context);
        FrameLayout.LayoutParams stickSize = new FrameLayout.LayoutParams(dp(144), dp(144), Gravity.CENTER);
        movement.addView(stick, stickSize);
        body.addView(movement, new LayoutParams(dp(156), -1));
        look = new LookPad(context);
        LayoutParams lookSize = new LayoutParams(0, -1, 1);
        lookSize.setMargins(dp(8), dp(6), dp(12), dp(6));
        body.addView(look, lookSize);

        LinearLayout actions = new LinearLayout(context);
        actions.setOrientation(VERTICAL);
        actions.setMotionEventSplittingEnabled(true);
        int[] labels = {R.string.nebula_console_attack, R.string.nebula_console_use,
            R.string.nebula_console_jump, R.string.nebula_console_sneak,
            R.string.nebula_console_inventory, R.string.nebula_console_drop};
        int[] codes = {0, 1, LwjglGlfwKeycode.GLFW_KEY_SPACE, LwjglGlfwKeycode.GLFW_KEY_LEFT_SHIFT,
            LwjglGlfwKeycode.GLFW_KEY_E, LwjglGlfwKeycode.GLFW_KEY_Q};
        for (int r = 0; r < 3; r++) {
            LinearLayout line = row();
            for (int c = 0; c < 2; c++) {
                int index = r * 2 + c;
                HoldButton button = new HoldButton(context, context.getString(labels[index]), codes[index], index < 2);
                LayoutParams size = new LayoutParams(0, -1, 1);
                size.setMargins(dp(2), dp(2), dp(2), dp(2));
                line.addView(button, size);
            }
            actions.addView(line, new LayoutParams(-1, 0, 1));
        }
        body.addView(actions, new LayoutParams(dp(180), -1));
        addView(body, new LayoutParams(-1, 0, 1));

        LinearLayout hotbar = row();
        for (int slot = 1; slot <= 9; slot++) {
            HoldButton button = new HoldButton(context, Integer.toString(slot), LwjglGlfwKeycode.GLFW_KEY_1 + slot - 1, false);
            button.setContentDescription(context.getString(R.string.nebula_console_slot, slot));
            LayoutParams size = new LayoutParams(0, -1, 1);
            size.setMargins(dp(2), dp(2), dp(2), 0);
            hotbar.addView(button, size);
        }
        addView(hotbar, new LayoutParams(-1, dp(50)));
    }

    public void cancelInput() {
        long time = android.os.SystemClock.uptimeMillis();
        MotionEvent cancel = MotionEvent.obtain(time, time, MotionEvent.ACTION_CANCEL, 0, 0, 0);
        dispatchTouchEvent(cancel);
        cancel.recycle();
        stick.reset();
        look.reset();
        input.release();
    }

    /** Each finger owns one control until up/cancel, including across rows. */
    @Override public boolean dispatchTouchEvent(MotionEvent event) {
        if (!onFilterTouchEventForSecurity(event)) return false;
        int action = event.getActionMasked();
        int index = event.getActionIndex();
        if (action == MotionEvent.ACTION_CANCEL || action == MotionEvent.ACTION_DOWN) {
            for (Finger finger : new ArrayList<>(fingers.values())) finger.send(event, MotionEvent.ACTION_CANCEL, -1);
            fingers.clear();
        }
        if (action == MotionEvent.ACTION_DOWN || action == MotionEvent.ACTION_POINTER_DOWN) {
            View target = targetAt(this, event.getX(index), event.getY(index));
            boolean owned = false;
            for (Finger finger : fingers.values()) if (finger.view == target) owned = true;
            if (target != null && !owned) {
                Finger finger = new Finger(target, event.getEventTime());
                fingers.put(event.getPointerId(index), finger);
                finger.send(event, MotionEvent.ACTION_DOWN, index);
                if (getParent() != null) getParent().requestDisallowInterceptTouchEvent(true);
            }
        } else if (action == MotionEvent.ACTION_MOVE) {
            for (Map.Entry<Integer, Finger> entry : new ArrayList<>(fingers.entrySet())) {
                int pointer = event.findPointerIndex(entry.getKey());
                entry.getValue().send(event, pointer < 0 ? MotionEvent.ACTION_CANCEL : MotionEvent.ACTION_MOVE, pointer);
                if (pointer < 0) fingers.remove(entry.getKey());
            }
        } else if (action == MotionEvent.ACTION_UP || action == MotionEvent.ACTION_POINTER_UP) {
            Finger finger = fingers.remove(event.getPointerId(index));
            if (finger != null) finger.send(event, MotionEvent.ACTION_UP, index);
        }
        if (fingers.isEmpty() && getParent() != null) getParent().requestDisallowInterceptTouchEvent(false);
        return true;
    }

    private View targetAt(ViewGroup parent, float x, float y) {
        for (int i = parent.getChildCount() - 1; i >= 0; i--) {
            View child = parent.getChildAt(i);
            if (child.getVisibility() != VISIBLE || !child.isEnabled()) continue;
            Rect bounds = new Rect();
            child.getDrawingRect(bounds);
            offsetDescendantRectToMyCoords(child, bounds);
            if (!bounds.contains((int)x, (int)y)) continue;
            if (child instanceof AppCompatButton || child instanceof Stick || child instanceof LookPad) return child;
            if (child instanceof ViewGroup) {
                View target = targetAt((ViewGroup)child, x, y);
                if (target != null) return target;
            }
        }
        return null;
    }

    private final class Finger {
        final View view;
        final long down;
        float x, y;
        Finger(View view, long down) { this.view = view; this.down = down; }
        void send(MotionEvent source, int action, int index) {
            if (index >= 0) {
                Rect bounds = new Rect();
                view.getDrawingRect(bounds);
                offsetDescendantRectToMyCoords(view, bounds);
                x = source.getX(index) - bounds.left;
                y = source.getY(index) - bounds.top;
            }
            MotionEvent single = MotionEvent.obtain(down, source.getEventTime(), action, x, y, 0);
            single.setSource(source.getSource());
            view.dispatchTouchEvent(single);
            single.recycle();
        }
    }

    public void grabChanged() {
        cancelInput();
        look.updateDescription();
        stick.setAlpha(game.grabbing() ? 1f : 0.4f);
    }

    public void safeInsets(int left, int right, int bottom) {
        setPadding(Math.max(dp(12), left), dp(4), Math.max(dp(12), right), Math.max(dp(8), bottom));
    }

    public int minimumPanelHeight() { return dp(262) + getPaddingBottom(); }

    private LinearLayout row() {
        LinearLayout row = new LinearLayout(getContext());
        row.setOrientation(HORIZONTAL);
        row.setMotionEventSplittingEnabled(true);
        return row;
    }

    private AppCompatButton action(String label, Runnable action) {
        AppCompatButton button = new AppCompatButton(getContext());
        style(button, label);
        button.setOnClickListener(v -> { input.release(); action.run(); });
        return button;
    }

    private void style(AppCompatButton button, String label) {
        button.setText(label);
        button.setAllCaps(false);
        button.setTextSize(12);
        button.setTextColor(INK);
        button.setMinWidth(0);
        button.setMinimumWidth(0);
        button.setMinHeight(0);
        button.setMinimumHeight(0);
        button.setPadding(dp(4), 0, dp(4), 0);
        button.setSingleLine(true);
        button.setFocusable(false); // Do not steal physical keyboard input from Minecraft.
        StateListDrawable background = new StateListDrawable();
        background.addState(new int[]{android.R.attr.state_pressed}, shape(0xff31584f));
        background.addState(new int[]{}, shape(0xff1e2b37));
        button.setBackground(background);
    }

    private GradientDrawable shape(int color) {
        GradientDrawable drawable = new GradientDrawable();
        drawable.setColor(color);
        drawable.setCornerRadius(dp(10));
        drawable.setStroke(dp(1), 0xff344454);
        return drawable;
    }

    private int dp(float value) { return Math.round(value * getResources().getDisplayMetrics().density); }

    private final class HoldButton extends AppCompatButton {
        private final int code;
        private final boolean mouse;
        private boolean held;
        HoldButton(Context context, String label, int code, boolean mouse) {
            super(context);
            this.code = code;
            this.mouse = mouse;
            style(this, label);
        }
        private void hold(boolean down) {
            if (held == down) return;
            held = down;
            setPressed(down);
            if (mouse) input.mouse(code, down); else input.key(code, down);
        }
        @SuppressLint("ClickableViewAccessibility")
        @Override public boolean onTouchEvent(MotionEvent event) {
            switch (event.getActionMasked()) {
                case MotionEvent.ACTION_DOWN: hold(true); break;
                case MotionEvent.ACTION_MOVE:
                    if (event.getX() < 0 || event.getX() > getWidth() || event.getY() < 0 || event.getY() > getHeight()) hold(false);
                    break;
                case MotionEvent.ACTION_UP:
                    boolean clicked = held;
                    hold(false);
                    if (clicked) super.performClick();
                    break;
                case MotionEvent.ACTION_CANCEL: hold(false); break;
            }
            return true;
        }
        @Override public boolean performClick() {
            super.performClick();
            hold(true);
            hold(false);
            return true;
        }
    }

    private final class Stick extends View {
        private final Paint paint = new Paint(Paint.ANTI_ALIAS_FLAG);
        private float x, y;
        private int pointer = -1;
        Stick(Context context) { super(context); setContentDescription(context.getString(R.string.nebula_console_move)); }
        void reset() { pointer = -1; move(0, 0); }
        private void move(float nextX, float nextY) {
            x = nextX; y = nextY;
            int horizontal = ConsoleInput.movement(x), vertical = ConsoleInput.movement(y);
            input.key(LwjglGlfwKeycode.GLFW_KEY_W, vertical < 0);
            input.key(LwjglGlfwKeycode.GLFW_KEY_S, vertical > 0);
            input.key(LwjglGlfwKeycode.GLFW_KEY_A, horizontal < 0);
            input.key(LwjglGlfwKeycode.GLFW_KEY_D, horizontal > 0);
            invalidate();
        }
        @SuppressLint("ClickableViewAccessibility")
        @Override public boolean onTouchEvent(MotionEvent event) {
            if (!game.grabbing()) { reset(); return true; }
            int action = event.getActionMasked();
            if (action == MotionEvent.ACTION_DOWN) pointer = event.getPointerId(0);
            if (action == MotionEvent.ACTION_UP || action == MotionEvent.ACTION_CANCEL ||
                (action == MotionEvent.ACTION_POINTER_UP && event.getPointerId(event.getActionIndex()) == pointer)) {
                reset(); return true;
            }
            int index = event.findPointerIndex(pointer);
            if (index < 0) return true;
            float radius = Math.min(getWidth(), getHeight()) * 0.34f;
            float dx = (event.getX(index) - getWidth() / 2f) / radius;
            float dy = (event.getY(index) - getHeight() / 2f) / radius;
            float length = Math.max(1f, (float)Math.hypot(dx, dy));
            move(dx / length, dy / length);
            return true;
        }
        @Override protected void onDraw(Canvas canvas) {
            float cx = getWidth() / 2f, cy = getHeight() / 2f, radius = Math.min(cx, cy) - dp(5);
            paint.setColor(0xff1e2b37); canvas.drawCircle(cx, cy, radius, paint);
            paint.setColor(0xff344454); paint.setStyle(Paint.Style.STROKE); paint.setStrokeWidth(dp(1));
            canvas.drawCircle(cx, cy, radius, paint); paint.setStyle(Paint.Style.FILL);
            paint.setColor(MUTED); paint.setTextSize(dp(13)); paint.setTextAlign(Paint.Align.CENTER);
            canvas.drawText("W", cx, cy - radius * 0.67f, paint);
            canvas.drawText("S", cx, cy + radius * 0.84f, paint);
            canvas.drawText("A", cx - radius * 0.76f, cy + dp(4), paint);
            canvas.drawText("D", cx + radius * 0.76f, cy + dp(4), paint);
            paint.setColor(ACCENT); canvas.drawCircle(cx + x * radius * 0.5f, cy + y * radius * 0.5f, radius * 0.32f, paint);
        }
    }

    private final class LookPad extends View {
        private final Paint paint = new Paint(Paint.ANTI_ALIAS_FLAG);
        private int pointer = -1;
        private float lastX, lastY, distance;
        private boolean menuTouch;
        LookPad(Context context) { super(context); setBackground(shape(0xff17222c)); updateDescription(); }
        void updateDescription() {
            setContentDescription(getContext().getString(game.grabbing() ? R.string.nebula_console_look : R.string.nebula_console_cursor));
            invalidate();
        }
        void reset() { pointer = -1; distance = 0; }
        @SuppressLint("ClickableViewAccessibility")
        @Override public boolean onTouchEvent(MotionEvent event) {
            int action = event.getActionMasked();
            if (action == MotionEvent.ACTION_DOWN) {
                pointer = event.getPointerId(0);
                lastX = event.getX(); lastY = event.getY(); distance = 0;
                menuTouch = !game.grabbing();
            } else if (action == MotionEvent.ACTION_MOVE) {
                int index = event.findPointerIndex(pointer);
                if (index < 0) return true;
                float dx = event.getX(index) - lastX, dy = event.getY(index) - lastY;
                lastX = event.getX(index); lastY = event.getY(index);
                distance += Math.hypot(dx, dy);
                if (menuTouch != !game.grabbing()) { reset(); return true; }
                game.move(dx, dy);
                invalidate();
            } else if (action == MotionEvent.ACTION_UP) {
                if (pointer != -1 && menuTouch && !game.grabbing() && distance < ViewConfiguration.get(getContext()).getScaledTouchSlop()) performClick();
                reset();
            } else if (action == MotionEvent.ACTION_CANCEL ||
                (action == MotionEvent.ACTION_POINTER_UP && event.getPointerId(event.getActionIndex()) == pointer)) reset();
            return true;
        }
        @Override public boolean performClick() {
            super.performClick();
            if (!game.grabbing()) { input.mouse(0, true); input.mouse(0, false); }
            return true;
        }
        @Override protected void onDraw(Canvas canvas) {
            paint.setColor(MUTED); paint.setTextSize(dp(12)); paint.setTextAlign(Paint.Align.CENTER);
            boolean grabbing = game.grabbing();
            String label = grabbing ? getContext().getString(R.string.nebula_console_look) : "Cursor";
            canvas.drawText(label, getWidth() / 2f, getHeight() / 2f - dp(4), paint);
            if (!grabbing) canvas.drawText(getContext().getString(R.string.nebula_console_tap), getWidth() / 2f, getHeight() / 2f + dp(16), paint);
            if (!game.grabbing()) {
                paint.setColor(ACCENT);
                float x = getWidth() * game.cursorX() / Math.max(1, game.width());
                float y = getHeight() * game.cursorY() / Math.max(1, game.height());
                canvas.drawCircle(Math.max(dp(5), Math.min(getWidth() - dp(5), x)), Math.max(dp(5), Math.min(getHeight() - dp(5), y)), dp(4), paint);
            }
        }
    }

    private static final class NativeGame implements Game {
        private final Touchpad cursor;
        NativeGame(Touchpad cursor) { this.cursor = cursor; }
        @Override public void key(int key, boolean down) { CallbackBridge.sendKeyPress(key, CallbackBridge.getCurrentMods(), down); }
        @Override public void mouse(int button, boolean down) { CallbackBridge.sendMouseButton(button, down); }
        @Override public boolean grabbing() { return CallbackBridge.isGrabbing(); }
        @Override public float cursorX() { return CallbackBridge.mouseX; }
        @Override public float cursorY() { return CallbackBridge.mouseY; }
        @Override public int width() { return CallbackBridge.windowWidth; }
        @Override public int height() { return CallbackBridge.windowHeight; }
        @Override public void keyboard() { MainActivity.touchCharInput.switchKeyboardState(); }
        @Override public void move(float dx, float dy) {
            if (grabbing()) {
                float sensitivity = 1.4f * 1080f / Math.max(1, CallbackBridge.physicalHeight);
                CallbackBridge.deltaX = dx * sensitivity;
                CallbackBridge.deltaY = dy * sensitivity;
                CallbackBridge.sendCursorPos(cursorX() + CallbackBridge.deltaX, cursorY() + CallbackBridge.deltaY);
            } else {
                float scale = Math.max(0.01f, LauncherPreferences.PREF_SCALE_FACTOR);
                float x = Math.max(0, Math.min(width() - 1, cursorX() + dx * scale));
                float y = Math.max(0, Math.min(height() - 1, cursorY() + dy * scale));
                cursor.placeMouseAt(x / scale, y / scale);
            }
        }
    }
}
