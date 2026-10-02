package io.unom.punktfunk.components

import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.graphics.vector.PathParser
import androidx.compose.ui.unit.dp
import kotlin.math.max

/**
 * One brand mark as a raw SVG path in its master's viewport — the shape both curated registries
 * ([launcherIcon], [resolveOsIcon]) hold, since Material ships no brand icons. [PathParser]
 * builds the vector once per token through [GlyphCache].
 */
class Glyph(val viewportWidth: Float, val viewportHeight: Float, val d: String)

/** A per-registry lazy [ImageVector] cache over a token → [Glyph] table. */
class GlyphCache(private val name: String, private val glyphs: Map<String, Glyph>) {
    private val built = HashMap<String, ImageVector>()

    /** The mark for [token], or null when the table has none. */
    operator fun get(token: String): ImageVector? =
        glyphs[token]?.let { glyph -> built.getOrPut(token) { glyph.build("$name.$token") } }
}

/**
 * The intrinsic size carries the VIEWPORT'S ASPECT RATIO, not a fixed square: a VectorPainter
 * maps the viewport onto the default size with independent x and y scales, so declaring a
 * 448x512 mark as 24x24 dp stretches it. The longest edge is 24 dp and Icon() paints with
 * ContentScale.Fit, so the mark letterboxes inside whatever box the caller sized. Fill colour
 * is irrelevant — Icon() tints via LocalContentColor, like Material icons.
 */
private fun Glyph.build(name: String): ImageVector {
    val longest = max(viewportWidth, viewportHeight)
    return ImageVector.Builder(
        name = name,
        defaultWidth = (24f * viewportWidth / longest).dp,
        defaultHeight = (24f * viewportHeight / longest).dp,
        viewportWidth = viewportWidth,
        viewportHeight = viewportHeight,
    ).apply {
        addPath(
            pathData = PathParser().parsePathString(d).toNodes(),
            fill = SolidColor(Color.Black),
        )
    }.build()
}
