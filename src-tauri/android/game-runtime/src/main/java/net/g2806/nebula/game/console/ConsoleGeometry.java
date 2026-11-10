package net.g2806.nebula.game.console;

/** Pixel geometry, independent of Android so posture edge cases can be tested. */
public final class ConsoleGeometry {
    private ConsoleGeometry() {}

    public static final class Split {
        public final int gameHeight, controlsTop;
        private Split(int gameHeight, int controlsTop) {
            this.gameHeight = gameHeight;
            this.controlsTop = controlsTop;
        }
    }

    public static Split resolve(int width, int height, int minWidth, int minGame,
                                int minControls, int gap, boolean forced,
                                boolean horizontalHalfOpen, boolean verticalSeparating,
                                int hingeTop, int hingeBottom) {
        if (width < minWidth || verticalSeparating || (!forced && !horizontalHalfOpen)) return null;
        int top = horizontalHalfOpen ? hingeTop : height / 2;
        int bottom = horizontalHalfOpen ? hingeBottom : height / 2;
        if (top < 0 || bottom < top || bottom > height) return null;
        int game = top - gap;
        int controls = bottom + gap;
        if (game < minGame || height - controls < minControls) return null;
        return new Split(game, controls);
    }
}
