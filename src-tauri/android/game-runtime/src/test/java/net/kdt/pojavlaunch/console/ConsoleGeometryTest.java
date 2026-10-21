package net.kdt.pojavlaunch.console;

import org.junit.Test;
import static org.junit.Assert.*;

public class ConsoleGeometryTest {
    private ConsoleGeometry.Split split(boolean forced, boolean half, boolean vertical, int top, int bottom) {
        return ConsoleGeometry.resolve(800, 720, 600, 140, 268, 8, forced, half, vertical, top, bottom);
    }
    @Test public void horizontalFoldKeepsBothPanesAwayFromHinge() {
        ConsoleGeometry.Split result = split(false, true, false, 348, 372);
        assertNotNull(result);
        assertEquals(340, result.gameHeight);
        assertEquals(380, result.controlsTop);
    }
    @Test public void flatAndBookPosturesDoNotActivateAutomatically() {
        assertNull(split(false, false, false, 0, 0));
        assertNull(split(false, false, true, 0, 720));
        assertNull(split(true, false, true, 0, 720));
    }
    @Test public void manualModeCanPreviewOnFlatDisplay() {
        ConsoleGeometry.Split result = split(true, false, false, 0, 0);
        assertNotNull(result);
        assertEquals(352, result.gameHeight);
        assertEquals(368, result.controlsTop);
    }
    @Test public void narrowCoverDisplayAndShortControlAreaStayFullscreen() {
        assertNull(ConsoleGeometry.resolve(480, 720, 600, 140, 268, 8, true, false, false, 0, 0));
        assertNull(split(false, true, false, 500, 500));
    }
    @Test public void RejectsHingesOutsideWindowOrInvalidBounds() {
        assertNull(split(false, true, false, -10, 10));
        assertNull(split(false, true, false, 350, 800));
        assertNull(split(false, true, false, 400, 300));
    }
}
