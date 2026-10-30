package org.libsdl.app;

/** Reads SDL's relative mouse state, which is package-private in the engine. */
public final class NebulaSdlMouse {
    private NebulaSdlMouse() {}

    public static boolean relative() {
        SDLGenericMotionListener_API14 listener = SDLActivity.getMotionListener();
        return listener != null && listener.inRelativeMode();
    }
}
