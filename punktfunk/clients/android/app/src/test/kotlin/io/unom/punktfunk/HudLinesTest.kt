package io.unom.punktfunk

import org.junit.Assert.assertEquals
import org.junit.Test

/** The wire between the native formatter and the overlay: role, tab, text, newline. */
class HudLinesTest {
    @Test
    fun decodesRolesAndDropsWhatIsNotALine() {
        val lines = decodeHudLines("0\t1920×1080@120 · HEVC 10-bit\n3\tlost 2 (0.8%)\nnot a line\n\n")
        assertEquals(
            listOf(HudLine(0, "1920×1080@120 · HEVC 10-bit"), HudLine(3, "lost 2 (0.8%)")),
            lines,
        )
        assertEquals(emptyList<HudLine>(), decodeHudLines(null))
        // A role code this build cannot read still draws, as a primary line.
        assertEquals(listOf(HudLine(0, "x")), decodeHudLines("x\tx\n"))
    }
}
