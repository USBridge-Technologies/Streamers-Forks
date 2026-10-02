package io.unom.punktfunk

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

/**
 * The one decision the background keep-alive makes before any Android object exists: whether a
 * backgrounded session holds, and for how long. Everything downstream — the service, the
 * notification, the countdown — reads its answer, so a wrong `null` here is a session that ends
 * when the user expected it to wait, and a wrong span is one the host can never reclaim.
 */
class StreamKeepAliveTest {
    @Test
    fun offIsTheDefault() {
        assertNull(keepAliveSpanMs(Settings(), isTv = false))
    }

    @Test
    fun onHoldsForTheChosenMinutes() {
        val s = Settings(backgroundKeepAlive = true, backgroundTimeoutMinutes = 5)
        assertEquals(5 * 60_000L, keepAliveSpanMs(s, isTv = false))
    }

    /** A TV has no notification shade, so the End action would be unreachable. */
    @Test
    fun aTvNeverHolds() {
        val s = Settings(backgroundKeepAlive = true, backgroundTimeoutMinutes = 10)
        assertNull(keepAliveSpanMs(s, isTv = true))
    }

    /** The minutes come out of a settings document, not only out of the picker. */
    @Test
    fun theSpanIsClamped() {
        fun span(minutes: Int) =
            keepAliveSpanMs(Settings(backgroundKeepAlive = true, backgroundTimeoutMinutes = minutes), false)
        assertEquals("zero would end the session on the way out", 60_000L, span(0))
        assertEquals(60_000L, span(-30))
        assertEquals(120 * 60_000L, span(100_000))
    }
}
