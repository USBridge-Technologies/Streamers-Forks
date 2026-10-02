package io.unom.punktfunk.kit

import kotlin.math.abs
import kotlin.math.floor
import kotlin.math.max
import kotlin.math.min

/**
 * Where a decoded frame lands in a view — the twin of `punktfunk_core::video_fit`. Every case in
 * `clients/shared/video-fit-vectors.json` runs against both; change the rule there first.
 *
 * [VideoPlacement.dstX]…[VideoPlacement.dstH] is the whole-pixel rect inside the view the picture
 * fills; [VideoPlacement.srcX]…[VideoPlacement.srcH] the part of the frame that stays visible. A
 * scale within [SNAP_PX] / [SNAP_REL] of a whole number snaps to it, so the frame shows 1:1 or
 * pixel-replicated instead of resampled by a hair.
 */
enum class VideoFit(val wire: String) {
    FIT("fit"),
    CROP("crop"),
    STRETCH("stretch");

    companion object {
        /** Unknown names read as [FIT], so a newer client's value degrades safely. */
        fun fromName(name: String?): VideoFit = entries.firstOrNull { it.wire == name } ?: FIT

        const val SNAP_PX = 8.0
        const val SNAP_REL = 0.005

        fun place(fit: VideoFit, viewW: Int, viewH: Int, frameW: Int, frameH: Int): VideoPlacement {
            if (viewW <= 0 || viewH <= 0 || frameW <= 0 || frameH <= 0) return VideoPlacement.EMPTY
            val sx0 = viewW.toDouble() / frameW
            val sy0 = viewH.toDouble() / frameH
            val long = max(frameW, frameH).toDouble()
            val (sx, sy) = when (fit) {
                FIT -> snap(min(sx0, sy0), long).let { it to it }
                CROP -> snap(max(sx0, sy0), long).let { it to it }
                STRETCH -> snap(sx0, frameW.toDouble()) to snap(sy0, frameH.toDouble())
            }
            val w = size(frameW, sx)
            val h = size(frameH, sy)
            val x0 = Math.floorDiv(viewW.toLong() - w, 2L)
            val y0 = Math.floorDiv(viewH.toLong() - h, 2L)
            val scaleX = w.toDouble() / frameW
            val scaleY = h.toDouble() / frameH
            val dstX = max(x0, 0L)
            val dstY = max(y0, 0L)
            val dstW = max(min(x0 + w, viewW.toLong()) - dstX, 0L)
            val dstH = max(min(y0 + h, viewH.toLong()) - dstY, 0L)
            return VideoPlacement(
                dstX.toInt(), dstY.toInt(), dstW.toInt(), dstH.toInt(),
                (dstX - x0) / scaleX, (dstY - y0) / scaleY, dstW / scaleX, dstH / scaleY,
                scaleX, scaleY,
            )
        }

        private fun snap(s: Double, len: Double): Double {
            val k = floor(s + 0.5)
            return if (k >= 1.0 && abs(k - s) <= max(SNAP_PX / len, SNAP_REL * k)) k else s
        }

        /** Rounded half away from zero, like the Rust twin; at least one pixel. */
        private fun size(frame: Int, scale: Double): Long = max(floor(frame * scale + 0.5).toLong(), 1L)
    }
}

enum class VideoKernel(val wire: String) {
    COPY("copy"),
    NEAREST("nearest"),
    CATMULL_ROM("catmull-rom"),
    LANCZOS("lanczos");

    companion object {
        fun of(scale: Double): VideoKernel = when {
            scale == 1.0 -> COPY
            scale > 1.0 && scale == floor(scale) -> NEAREST
            scale > 1.0 -> CATMULL_ROM
            else -> LANCZOS
        }
    }
}

data class VideoPlacement(
    val dstX: Int,
    val dstY: Int,
    val dstW: Int,
    val dstH: Int,
    val srcX: Double,
    val srcY: Double,
    val srcW: Double,
    val srcH: Double,
    /** View pixels per frame pixel. Exact whole numbers when snapped. */
    val scaleX: Double,
    val scaleY: Double,
) {
    val isEmpty: Boolean get() = dstW == 0 || dstH == 0
    val kernelX: VideoKernel get() = VideoKernel.of(scaleX)
    val kernelY: VideoKernel get() = VideoKernel.of(scaleY)

    /** View x → frame x, clamped onto the visible region. */
    fun frameX(viewX: Double): Double = (srcX + (viewX - dstX) / scaleX).coerceIn(srcX, srcX + srcW)

    /** View y → frame y, clamped onto the visible region. */
    fun frameY(viewY: Double): Double = (srcY + (viewY - dstY) / scaleY).coerceIn(srcY, srcY + srcH)

    companion object {
        val EMPTY = VideoPlacement(0, 0, 0, 0, 0.0, 0.0, 0.0, 0.0, 1.0, 1.0)
    }
}
