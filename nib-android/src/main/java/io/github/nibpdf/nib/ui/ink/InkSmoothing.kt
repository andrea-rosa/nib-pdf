package io.github.nibpdf.nib.ui.ink

import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Path
import kotlin.math.hypot
import kotlin.math.max

/**
 * Ink input pipeline (JSON_SCHEMA §5.1 `smoothing`): resample → RDP
 * simplify → Catmull-Rom rendering path.
 */
internal object InkSmoothing {
    /** One pointer sample; [pressure] ∈ [0,1], [tMs] ms since stroke start. */
    data class Sample(
        val x: Float,
        val y: Float,
        val pressure: Float,
        val tMs: Long,
    )

    /**
     * Keeps the first sample, then any sample ≥ [spacing] pt from the last
     * kept one, and always the last sample.
     */
    fun resample(
        samples: List<Sample>,
        spacing: Float,
    ): List<Sample> {
        if (samples.isEmpty()) return emptyList()
        val out = ArrayList<Sample>(samples.size)
        var last = samples.first()
        out += last
        for (s in samples.drop(1).dropLast(1)) {
            if (hypot(s.x - last.x, s.y - last.y) >= spacing) {
                out += s
                last = s
            }
        }
        if (samples.size > 1) out += samples.last()
        return out
    }

    /** Iterative Ramer–Douglas–Peucker on (x, y); endpoints always kept. */
    fun rdp(
        points: List<Sample>,
        tolerance: Float,
    ): List<Sample> {
        if (points.size <= 2) return points.toList()
        val keep = BooleanArray(points.size)
        keep[0] = true
        keep[points.size - 1] = true
        val stack = ArrayDeque<IntRange>()
        stack += 0 until points.size
        while (stack.isNotEmpty()) {
            val range = stack.removeLast()
            if (range.last - range.first < 2) continue
            val a = points[range.first]
            val b = points[range.last]
            var maxDist = -1f
            var maxIdx = -1
            for (i in range.first + 1 until range.last) {
                val p = points[i]
                val d = perpendicularDistance(p, a, b)
                if (d > maxDist) {
                    maxDist = d
                    maxIdx = i
                }
            }
            if (maxDist > tolerance && maxIdx >= 0) {
                keep[maxIdx] = true
                stack += range.first..maxIdx
                stack += maxIdx..range.last
            }
        }
        return points.filterIndexed { i, _ -> keep[i] }
    }

    private fun perpendicularDistance(
        p: Sample,
        a: Sample,
        b: Sample,
    ): Float {
        val dx = b.x - a.x
        val dy = b.y - a.y
        val len = hypot(dx.toDouble(), dy.toDouble()).toFloat()
        if (len == 0f) return hypot((p.x - a.x).toDouble(), (p.y - a.y).toDouble()).toFloat()
        return max(0f, kotlin.math.abs(dy * p.x - dx * p.y + b.x * a.y - b.y * a.x) / len)
    }

    /**
     * Catmull-Rom spline through [points] as cubic Béziers.
     * [tension] ∈ [0,1] scales the control-point arm (0.5 = centripetal-ish
     * default, 0 = straight polyline).
     */
    fun catmullRomPath(
        points: List<Offset>,
        tension: Float = 0.5f,
    ): Path {
        val path = Path()
        if (points.isEmpty()) return path
        if (points.size == 1) {
            path.addOval(
                androidx.compose.ui.geometry.Rect(
                    points[0] - Offset(0.5f, 0.5f),
                    points[0] + Offset(0.5f, 0.5f),
                ),
            )
            return path
        }
        path.moveTo(points[0].x, points[0].y)
        if (points.size == 2) {
            path.lineTo(points[1].x, points[1].y)
            return path
        }
        val t = tension / 3f
        for (i in 0 until points.size - 1) {
            val p0 = points[max(0, i - 1)]
            val p1 = points[i]
            val p2 = points[i + 1]
            val p3 = points[(i + 2).coerceAtMost(points.size - 1)]
            val c1 = Offset(p1.x + (p2.x - p0.x) * t, p1.y + (p2.y - p0.y) * t)
            val c2 = Offset(p2.x - (p3.x - p1.x) * t, p2.y - (p3.y - p1.y) * t)
            path.cubicTo(c1.x, c1.y, c2.x, c2.y, p2.x, p2.y)
        }
        return path
    }
}
