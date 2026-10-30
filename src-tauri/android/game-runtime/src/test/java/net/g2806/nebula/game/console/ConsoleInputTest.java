package net.g2806.nebula.game.console;

import java.util.ArrayList;
import java.util.List;
import org.junit.Test;
import static org.junit.Assert.*;

public class ConsoleInputTest {
    private final List<String> events = new ArrayList<>();
    private final ConsoleInput input = new ConsoleInput(new ConsoleInput.Sink() {
        @Override public void key(int key, boolean down) { events.add("key:" + key + ":" + down); }
        @Override public void mouse(int button, boolean down) { events.add("mouse:" + button + ":" + down); }
    });
    @Test public void repeatedMovementDoesNotRepeatKeyDown() {
        input.key(87, true); input.key(87, true); input.key(87, false); input.key(87, false);
        assertEquals(java.util.Arrays.asList("key:87:true", "key:87:false"), events);
    }
    @Test public void pauseOrFoldReleasesEveryOwnedInputExactlyOnce() {
        input.key(87, true); input.key(65, true); input.mouse(0, true);
        input.release(); input.release();
        assertEquals(6, events.size());
        assertTrue(events.contains("key:87:false"));
        assertTrue(events.contains("key:65:false"));
        assertTrue(events.contains("mouse:0:false"));
        assertFalse(events.contains("key:32:false"));
    }
    @Test public void joystickDeadZoneAndDirectionChanges() {
        assertEquals(0, ConsoleInput.movement(0.2f));
        assertEquals(0, ConsoleInput.movement(-0.2f));
        assertEquals(-1, ConsoleInput.movement(-0.8f));
        assertEquals(1, ConsoleInput.movement(0.8f));
    }
}
