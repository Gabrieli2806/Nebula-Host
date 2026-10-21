package net.kdt.pojavlaunch.console;

import java.util.HashSet;
import java.util.Set;

/** Track console presses so cancellation releases only codes it actually held. */
public final class ConsoleInput {
    public interface Sink {
        void key(int key, boolean down);
        void mouse(int button, boolean down);
    }
    private final Sink sink;
    private final Set<Integer> keys = new HashSet<>(), buttons = new HashSet<>();

    public ConsoleInput(Sink sink) { this.sink = sink; }
    public void key(int key, boolean down) {
        if (down ? keys.add(key) : keys.remove(key)) sink.key(key, down);
    }
    public void mouse(int button, boolean down) {
        if (down ? buttons.add(button) : buttons.remove(button)) sink.mouse(button, down);
    }
    public void release() {
        for (int key : new HashSet<>(keys)) key(key, false);
        for (int button : new HashSet<>(buttons)) mouse(button, false);
    }
    public static int movement(float value) {
        return value < -0.25f ? -1 : value > 0.25f ? 1 : 0;
    }
}
