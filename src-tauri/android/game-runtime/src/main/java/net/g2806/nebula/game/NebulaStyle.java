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
    /** The tabletop console pad is true black on neutral greys, for OLED screens. */
    public static final int CONSOLE_PANEL = 0xFF000000;
    public static final int CONSOLE_KEY = 0xFF161616;
    public static final int CONSOLE_KEY_PRESSED = 0xFF2C2C2C;
    public static final int CONSOLE_OUTLINE = 0x33FFFFFF;
    public static final int CONSOLE_MUTED = 0xFFA3A3A3;

    private NebulaStyle() {}
}
