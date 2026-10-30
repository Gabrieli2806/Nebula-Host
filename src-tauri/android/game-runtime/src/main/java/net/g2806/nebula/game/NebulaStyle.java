package net.g2806.nebula.game;

/** Nebula Host's colours (src/styles/tokens.css, dark theme) for the game's
 *  own screens and on-screen controls, so they read as part of the app. */
public final class NebulaStyle {
    /** --ink surface behind the game. */
    public static final int SURFACE = 0xFF0B1220;
    /** --accent-base (#22d3ee). */
    public static final int ACCENT = 0xFF22D3EE;
    /** Secondary text on the dark surface. */
    public static final int TEXT_MUTED = 0xFFA9B4C6;
    /** Control buttons: the surface at 55%, the accent outline at 50%. */
    public static final int CONTROL_FILL = 0x8C0B1220;
    public static final int CONTROL_OUTLINE = 0x8022D3EE;
    /** Console pad keys: a raised well above the surface, brighter while held. */
    public static final int RAISED = 0xFF16213A;
    public static final int PRESSED = 0xFF1F3A55;

    private NebulaStyle() {}
}
