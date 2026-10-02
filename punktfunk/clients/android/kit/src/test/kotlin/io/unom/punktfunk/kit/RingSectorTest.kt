package io.unom.punktfunk.kit

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test
import kotlin.math.cos
import kotlin.math.sin

/**
 * The ring's stick aiming (design/touch-client-overlay.md §2.6): the thumb points at a slot rather
 * than stepping one disc per push. Pinned here because the geometry is duplicated in three
 * languages — `pf_client_core::menu_nav::ring_sector` is the reference and the Apple client pins
 * its own half (`RingSectorTests`) — and a drift in the +y sense or the 12-o'clock origin turns
 * "aim at Disconnect" into "aim at Keyboard", silently, on a couch nobody can debug from.
 */
class RingSectorTest {

    private fun at(deg: Double, out: Float = 1f): Pair<Float, Float> {
        val rad = Math.toRadians(deg - 90.0)
        return Pair((out * cos(rad)).toFloat(), (out * sin(rad)).toFloat())
    }

    /**
     * `MotionEvent.AXIS_Y` is +down, which is the screen's own sense; slot k sits at
     * `-90° + 60°·k`, 12 o'clock first, going clockwise.
     */
    @Test
    fun `the thumb points at the slot under it`() {
        for (k in 0..5) {
            val (x, y) = at(60.0 * k)
            assertEquals("slot $k", k, GamepadRouter.ringSector(x, y, null))
        }
        assertEquals(0, GamepadRouter.ringSector(0f, -1f, null))
        assertEquals(3, GamepadRouter.ringSector(0f, 1f, null))
    }

    /** A resting thumb owns no slot: the ring goes back to its centre, and drift never aims. */
    @Test
    fun `neutral owns nothing`() {
        assertNull(GamepadRouter.ringSector(0f, 0f, null))
        assertNull(GamepadRouter.ringSector(0.4f, 0.2f, null))
        // A diagonal counts by magnitude — 0.4/0.4 is past 0.5 out, though neither axis is.
        assertEquals(1, GamepadRouter.ringSector(0.4f, -0.4f, null))
    }

    /**
     * The engaged sector holds past its 30° edge, so a thumb parked on a boundary cannot flicker
     * between two discs; the looser release floor keeps it engaged as the stick eases back.
     */
    @Test
    fun `an engaged sector holds the boundary`() {
        val (x, y) = at(32.0)
        assertEquals(1, GamepadRouter.ringSector(x, y, null))
        assertEquals(0, GamepadRouter.ringSector(x, y, 0))
        // 40° past is nobody's boundary case — the overlap is 5°.
        val (fx, fy) = at(40.0)
        assertEquals(1, GamepadRouter.ringSector(fx, fy, 0))
        // Eased back to 0.4 out: too weak to engage, strong enough to keep what it had.
        val (wx, wy) = at(0.0, 0.4f)
        assertNull(GamepadRouter.ringSector(wx, wy, null))
        assertEquals(0, GamepadRouter.ringSector(wx, wy, 0))
    }
}
