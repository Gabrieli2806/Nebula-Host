package net.g2806.nebula.game.console;

import android.content.Context;
import android.graphics.RectF;
import android.view.ContextThemeWrapper;
import android.view.InputDevice;
import android.view.MotionEvent;
import android.view.View;
import android.view.ViewGroup;
import android.widget.Button;
import java.util.ArrayList;
import java.util.HashSet;
import java.util.List;
import java.util.Set;
import net.kdt.pojavlaunch.R;
import org.junit.Before;
import org.junit.Test;
import org.junit.runner.RunWith;
import org.robolectric.RobolectricTestRunner;
import org.robolectric.RuntimeEnvironment;
import org.robolectric.annotation.Config;
import static org.junit.Assert.*;

@RunWith(RobolectricTestRunner.class)
@Config(sdk = 36, qualifiers = "w800dp-h720dp-land-mdpi")
public class ConsolePadTest {
    private final FakeGame game = new FakeGame();
    private ConsolePad pad;
    private Context context;
    private long time;

    @Before public void createPad() {
        context = new ContextThemeWrapper(RuntimeEnvironment.getApplication(), R.style.AppTheme);
        pad = new ConsolePad(context, game, () -> {});
        layout(600, 270); // Smallest supported panel, in mdpi pixels.
    }
    private void layout(int width, int height) {
        pad.measure(View.MeasureSpec.makeMeasureSpec(width, View.MeasureSpec.EXACTLY), View.MeasureSpec.makeMeasureSpec(height, View.MeasureSpec.EXACTLY));
        pad.layout(0, 0, width, height);
    }

    @Test public void allButtonsFitAndHave48dpTouchTargetsAtMinimumSize() {
        List<View> buttons = new ArrayList<>();
        collectButtons(pad, buttons);
        assertEquals(18, buttons.size()); // 3 toolbar, 6 actions, 9 slots.
        for (View button : buttons) {
            RectF bounds = bounds(button);
            assertTrue(button + " width", bounds.width() >= 48);
            assertTrue(button + " height", bounds.height() >= 48);
            assertTrue(button + " outside panel", bounds.left >= 0 && bounds.top >= 0 && bounds.right <= pad.getWidth() && bounds.bottom <= pad.getHeight());
        }
    }

    @Test public void attackAndJumpCanBeHeldIndependentlyWithTwoFingers() {
        RectF attack = bounds(button(R.string.nebula_console_attack));
        RectF jump = bounds(button(R.string.nebula_console_jump));
        touch(MotionEvent.ACTION_DOWN, attack);
        touch(MotionEvent.ACTION_POINTER_DOWN | (1 << MotionEvent.ACTION_POINTER_INDEX_SHIFT), attack, jump);
        assertTrue(game.mouse.contains(0));
        assertTrue(game.keys.contains(32));
        touch(MotionEvent.ACTION_POINTER_UP, attack, jump);
        assertFalse(game.mouse.contains(0));
        assertTrue(game.keys.contains(32));
        pad.cancelInput();
        assertTrue(game.keys.isEmpty());
        assertFalse(button(R.string.nebula_console_jump).isPressed());
    }

    @Test public void joystickSupportsDiagonalMovementAndCancellation() {
        View stick = description(pad, context.getString(R.string.nebula_console_move));
        RectF area = bounds(stick);
        RectF corner = new RectF(area.left + 25, area.top + 25, area.left + 25, area.top + 25);
        touch(MotionEvent.ACTION_DOWN, corner);
        assertTrue(game.keys.contains(87));
        assertTrue(game.keys.contains(65));
        pad.cancelInput();
        assertTrue(game.keys.isEmpty());
    }

    @Test public void draggingLookPadMovesCameraWithoutAttacking() {
        View look = description(pad, context.getString(R.string.nebula_console_look));
        RectF start = bounds(look);
        touch(MotionEvent.ACTION_DOWN, start);
        RectF moved = new RectF(start); moved.offset(28, 15);
        touch(MotionEvent.ACTION_MOVE, moved);
        touch(MotionEvent.ACTION_UP, moved);
        assertEquals(28f, game.dx, 0.01f);
        assertEquals(15f, game.dy, 0.01f);
        assertEquals(0, game.clicks);
    }

    @Test public void menuTapSelectsAndGrabTransitionCancelsHeldButtons() {
        game.grabbing = false;
        pad.grabChanged();
        View look = description(pad, context.getString(R.string.nebula_console_cursor));
        touch(MotionEvent.ACTION_DOWN, bounds(look));
        touch(MotionEvent.ACTION_UP, bounds(look));
        assertEquals(1, game.clicks);
        touch(MotionEvent.ACTION_DOWN, bounds(button(R.string.nebula_console_use)));
        assertTrue(game.mouse.contains(1));
        game.grabbing = true;
        pad.grabChanged();
        assertTrue(game.mouse.isEmpty());
    }

    @Test public void cameraFingerCanLiftWhileJoystickRemainsHeld() {
        RectF joystick = bounds(description(pad, context.getString(R.string.nebula_console_move)));
        RectF forward = new RectF(joystick.centerX(), joystick.top + 25, joystick.centerX(), joystick.top + 25);
        RectF camera = bounds(description(pad, context.getString(R.string.nebula_console_look)));
        touch(MotionEvent.ACTION_DOWN, forward);
        touch(MotionEvent.ACTION_POINTER_DOWN | (1 << MotionEvent.ACTION_POINTER_INDEX_SHIFT), forward, camera);
        RectF moved = new RectF(camera); moved.offset(20, 0);
        touch(MotionEvent.ACTION_MOVE, forward, moved);
        assertEquals(20f, game.dx, 0.01f);
        touch(MotionEvent.ACTION_POINTER_UP | (1 << MotionEvent.ACTION_POINTER_INDEX_SHIFT), forward, moved);
        assertTrue(game.keys.contains(87));
        pad.cancelInput();
        assertTrue(game.keys.isEmpty());
    }

    @Test public void gestureInsetsKeepButtonsAboveSystemGestureArea() {
        pad.safeInsets(16, 16, 24);
        layout(600, pad.minimumPanelHeight());
        assertEquals(286, pad.getHeight());
        List<View> buttons = new ArrayList<>(); collectButtons(pad, buttons);
        for (View button : buttons) {
            RectF rect = bounds(button);
            assertTrue(rect.height() >= 48);
            assertTrue(rect.bottom <= pad.getHeight() - 24);
        }
    }

    private View button(int label) {
        List<View> buttons = new ArrayList<>(); collectButtons(pad, buttons);
        for (View view : buttons) if (((Button)view).getText().toString().equals(context.getString(label))) return view;
        throw new AssertionError("Button missing");
    }
    private void collectButtons(ViewGroup parent, List<View> result) {
        for (int i = 0; i < parent.getChildCount(); i++) {
            View view = parent.getChildAt(i);
            if (view instanceof Button) result.add(view);
            if (view instanceof ViewGroup) collectButtons((ViewGroup)view, result);
        }
    }
    private View description(ViewGroup parent, String text) {
        for (int i = 0; i < parent.getChildCount(); i++) {
            View view = parent.getChildAt(i);
            if (text.contentEquals(view.getContentDescription() == null ? "" : view.getContentDescription())) return view;
            if (view instanceof ViewGroup) {
                View found = description((ViewGroup)view, text);
                if (found != null) return found;
            }
        }
        return null;
    }
    private RectF bounds(View view) {
        assertNotNull(view);
        float x = 0, y = 0;
        View current = view;
        while (current != pad) { x += current.getLeft(); y += current.getTop(); current = (View)current.getParent(); }
        return new RectF(x, y, x + view.getWidth(), y + view.getHeight());
    }
    private void touch(int action, RectF... positions) {
        MotionEvent.PointerProperties[] properties = new MotionEvent.PointerProperties[positions.length];
        MotionEvent.PointerCoords[] coords = new MotionEvent.PointerCoords[positions.length];
        for (int i = 0; i < positions.length; i++) {
            properties[i] = new MotionEvent.PointerProperties(); properties[i].id = i + 10; properties[i].toolType = MotionEvent.TOOL_TYPE_FINGER;
            coords[i] = new MotionEvent.PointerCoords(); coords[i].x = positions[i].centerX(); coords[i].y = positions[i].centerY(); coords[i].pressure = 1; coords[i].size = 1;
        }
        MotionEvent event = MotionEvent.obtain(0, ++time * 20, action, positions.length, properties, coords, 0, 0, 1, 1, 0, 0, InputDevice.SOURCE_TOUCHSCREEN, 0);
        pad.dispatchTouchEvent(event);
        event.recycle();
    }

    private static class FakeGame implements ConsolePad.Game {
        boolean grabbing = true;
        Set<Integer> keys = new HashSet<>(), mouse = new HashSet<>();
        float dx, dy;
        int clicks;
        @Override public void key(int key, boolean down) { if (down) keys.add(key); else keys.remove(key); }
        @Override public void mouse(int button, boolean down) { if (down) { mouse.add(button); if (button == 0) clicks++; } else mouse.remove(button); }
        @Override public boolean grabbing() { return grabbing; }
        @Override public void move(float dx, float dy) { this.dx += dx; this.dy += dy; }
        @Override public float cursorX() { return 100; }
        @Override public float cursorY() { return 100; }
        @Override public int width() { return 800; }
        @Override public int height() { return 340; }
        @Override public void keyboard() {}
    }
}
