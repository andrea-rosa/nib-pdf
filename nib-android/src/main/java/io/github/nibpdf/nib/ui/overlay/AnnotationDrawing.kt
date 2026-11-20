package io.github.nibpdf.nib.ui.overlay

import android.graphics.BitmapFactory
import android.graphics.Paint
import android.graphics.Typeface
import android.util.Base64
import androidx.collection.LruCache
import androidx.compose.ui.geometry.CornerRadius
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.BlendMode
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.ImageBitmap
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.PathEffect
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.graphics.drawscope.DrawScope
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.graphics.drawscope.drawIntoCanvas
import androidx.compose.ui.graphics.drawscope.withTransform
import androidx.compose.ui.graphics.nativeCanvas
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.TextMeasurer
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.drawText
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.Constraints
import androidx.compose.ui.unit.IntOffset
import androidx.compose.ui.unit.IntSize
import androidx.compose.ui.unit.TextUnit
import androidx.compose.ui.unit.TextUnitType
import io.github.nibpdf.nib.delta.Annotation
import io.github.nibpdf.nib.ui.PageTransform
import io.github.nibpdf.nib.ui.ink.InkSmoothing
import io.github.nibpdf.nib.ui.toComposeColor
import kotlinx.serialization.json.contentOrNull
import kotlinx.serialization.json.jsonArray
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import kotlin.math.PI
import kotlin.math.atan2
import kotlin.math.cos
import kotlin.math.min
import kotlin.math.sin

/** LRU cache of decoded stamp bitmaps, keyed by `image.sha256` (or id). */
internal class StampImageCache {
    private val cache = LruCache<String, ImageBitmap>(16)

    fun get(
        key: String,
        base64: String?,
    ): ImageBitmap? {
        cache[key]?.let { return it }
        val bytes =
            base64?.let { runCatching { Base64.decode(it, Base64.DEFAULT) }.getOrNull() }
                ?: return null
        val bmp = BitmapFactory.decodeByteArray(bytes, 0, bytes.size) ?: return null
        val img = bmp.asImageBitmap()
        cache.put(key, img)
        return img
    }
}

/**
 * Reference renderer for committed annotations (JSON_SCHEMA §5). All
 * coordinates come in user-space points and are mapped through [t] to
 * content px; stroke widths and fonts scale by [k] (content px per pt).
 */
internal object AnnotationDrawing {
    private val DASH = PathEffect.dashPathEffect(floatArrayOf(8f, 6f))

    fun drawAnnotation(
        scope: DrawScope,
        a: Annotation,
        t: PageTransform,
        k: Float,
        textMeasurer: TextMeasurer,
        images: StampImageCache,
        offsetUser: Offset = Offset.Zero,
    ) {
        if (a.flagsHidden) return
        // User-space (dx, dy) → content-space (dx*k, −dy*k).
        with(scope) {
            withTransform({ translate(offsetUser.x * k, -offsetUser.y * k) }) {
                when (a.subtype) {
                    "Ink" -> drawInk(a, t, k)
                    "Highlight" -> drawMarkup(a, t, k, Markup.Highlight)
                    "Underline" -> drawMarkup(a, t, k, Markup.Underline)
                    "StrikeOut" -> drawMarkup(a, t, k, Markup.StrikeOut)
                    "Squiggly" -> drawMarkup(a, t, k, Markup.Squiggly)
                    "FreeText" -> drawFreeText(a, t, k, textMeasurer)
                    "Text" -> drawStickyNote(a, t, k)
                    "Stamp" -> drawStamp(a, t, k, textMeasurer, images)
                    "Square" -> drawRectShape(a, t, k, oval = false)
                    "Circle" -> drawRectShape(a, t, k, oval = true)
                    "Line" -> drawLine(a, t, k)
                    else ->
                        a.rect?.let { r ->
                            drawRect(
                                color = (a.color ?: "#888888").toComposeColor(a.opacity),
                                topLeft = t.toContent(Offset(r.left, r.bottom)),
                                size = Size((r.right - r.left) * k, (r.bottom - r.top) * k),
                                style = Stroke(width = k),
                            )
                        }
                }
            }
        }
    }

    private fun DrawScope.urect(
        a: Annotation,
        t: PageTransform,
    ): Rect? = a.rect?.let { t.rectToContent(Rect(it.left, it.top, it.right, it.bottom)) }

    private fun DrawScope.upt(
        t: PageTransform,
        x: Float,
        y: Float,
    ): Offset = t.toContent(Offset(x, y))

    private fun dash(a: Annotation): PathEffect? = if (a.borderStyle == "dashed") DASH else null

    // ---- Ink -------------------------------------------------------------

    private fun DrawScope.drawInk(
        a: Annotation,
        t: PageTransform,
        k: Float,
    ) {
        val color = (a.color ?: "#1A237E").toComposeColor(a.opacity)
        val baseW = (a.strokeWidth ?: 2f) * k
        val strokes = a.strokes ?: return
        strokes.forEach { stroke ->
            if (stroke.isEmpty()) return@forEach
            if (stroke.size == 1) {
                val point = stroke.first()
                val factor =
                    if (a.pressureSensitive && point.size >= 3) {
                        lerp(a.minWidthFactor, a.maxWidthFactor, point[2])
                    } else {
                        1f
                    }
                drawCircle(color, baseW * factor / 2f, upt(t, point[0], point[1]))
                return@forEach
            }
            if (a.pressureSensitive && stroke.first().size >= 3) {
                // Per-segment spans, width interpolated from avg endpoint pressure.
                for (i in 0 until stroke.size - 1) {
                    val p0 = stroke[i]
                    val p1 = stroke[i + 1]
                    val w = baseW * lerp(a.minWidthFactor, a.maxWidthFactor, (p0[2] + p1[2]) / 2f)
                    drawLine(
                        color = color,
                        start = upt(t, p0[0], p0[1]),
                        end = upt(t, p1[0], p1[1]),
                        strokeWidth = w,
                        cap = androidx.compose.ui.graphics.StrokeCap.Round,
                    )
                }
            } else {
                val path =
                    if (a.smoothingAlgorithm == "catmullRom") {
                        InkSmoothing.catmullRomPath(
                            stroke.map { upt(t, it[0], it[1]) },
                            a.smoothingTension,
                        )
                    } else {
                        Path().apply {
                            val first = upt(t, stroke[0][0], stroke[0][1])
                            moveTo(first.x, first.y)
                            stroke.drop(1).forEach { p ->
                                val c = upt(t, p[0], p[1])
                                lineTo(c.x, c.y)
                            }
                        }
                    }
                drawPath(
                    path,
                    color = color,
                    style =
                        Stroke(
                            width = baseW,
                            cap = androidx.compose.ui.graphics.StrokeCap.Round,
                            join = androidx.compose.ui.graphics.StrokeJoin.Round,
                        ),
                )
            }
        }
    }

    // ---- Markup -----------------------------------------------------------

    private enum class Markup { Highlight, Underline, StrikeOut, Squiggly }

    private fun DrawScope.drawMarkup(
        a: Annotation,
        t: PageTransform,
        k: Float,
        kind: Markup,
    ) {
        val color = (a.color ?: "#FFEB3B").toComposeColor(a.opacity)
        val quads = a.quadPoints ?: return
        quads.forEach { q ->
            if (q.size < 8) return@forEach
            // Stored order: UL(x0,y1) UR(x1,y1) LL(x0,y0) LR(x1,y0), y-up.
            val ul = upt(t, q[0], q[1])
            val ur = upt(t, q[2], q[3])
            val ll = upt(t, q[4], q[5])
            val lr = upt(t, q[6], q[7])
            val quadH = ll.y - ul.y
            when (kind) {
                Markup.Highlight -> {
                    val path =
                        Path().apply {
                            moveTo(ul.x, ul.y)
                            lineTo(ur.x, ur.y)
                            lineTo(lr.x, lr.y)
                            lineTo(ll.x, ll.y)
                            close()
                        }
                    val blend = if (a.blendMode == "multiply") BlendMode.Multiply else BlendMode.SrcOver
                    drawPath(path, color = color, blendMode = blend)
                }
                else -> {
                    val thickness = a.lineWidthFactor * quadH
                    val y =
                        when (kind) {
                            Markup.Underline -> ll.y - thickness / 2f
                            Markup.StrikeOut -> (ul.y + ll.y) / 2f
                            Markup.Squiggly -> ll.y - thickness / 2f
                            Markup.Highlight -> 0f
                        }
                    if (kind == Markup.Squiggly) {
                        val path = Path()
                        val period = 2f * thickness
                        var x = ll.x
                        var up = true
                        path.moveTo(x, y)
                        while (x < lr.x) {
                            x = min(x + period / 2f, lr.x)
                            path.lineTo(x, if (up) y - thickness else y)
                            up = !up
                        }
                        drawPath(path, color = color, style = Stroke(width = thickness / 2f))
                    } else {
                        drawLine(
                            color = color,
                            start = Offset(ll.x, y),
                            end = Offset(lr.x, y),
                            strokeWidth = thickness,
                        )
                    }
                }
            }
        }
    }

    // ---- FreeText ----------------------------------------------------------

    private fun DrawScope.drawFreeText(
        a: Annotation,
        t: PageTransform,
        k: Float,
        textMeasurer: TextMeasurer,
    ) {
        val r = urect(a, t) ?: return
        val pad = (a.json["padding"]?.toString()?.toFloatOrNull() ?: 2f) * k
        a.fillColor?.let { drawRect(color = it.toComposeColor(a.opacity), topLeft = r.topLeft, size = r.size) }
        a.borderColor?.let { c ->
            drawRect(
                color = c.toComposeColor(a.opacity),
                topLeft = r.topLeft,
                size = r.size,
                style = Stroke(width = (a.borderWidth ?: 1f) * k, pathEffect = dash(a)),
            )
        }
        val text = a.contents ?: return
        val align =
            when (a.textAlign) {
                "center" -> TextAlign.Center
                "right" -> TextAlign.Right
                else -> TextAlign.Left
            }
        val style =
            TextStyle(
                fontSize = TextUnit(a.fontSize * k / density, TextUnitType.Sp),
                color = (a.color ?: "#000000").toComposeColor(a.opacity),
                textAlign = align,
            )
        val laid =
            textMeasurer.measure(
                AnnotatedString(text),
                style = style,
                constraints = Constraints(maxWidth = (r.width - 2 * pad).toInt().coerceAtLeast(1)),
            )
        drawText(laid, topLeft = Offset(r.left + pad, r.top + pad))
    }

    // ---- Sticky note -------------------------------------------------------

    private fun DrawScope.drawStickyNote(
        a: Annotation,
        t: PageTransform,
        k: Float,
    ) {
        val anchor = a.rect?.let { upt(t, it.left, it.bottom) } ?: return
        val size = 20f * k
        val color = (a.color ?: "#FFC107").toComposeColor(a.opacity)
        drawRoundRect(
            color = color,
            topLeft = anchor,
            size = Size(size, size),
            cornerRadius = CornerRadius(3f * k),
        )
        drawRoundRect(
            color = Color.White,
            topLeft = anchor,
            size = Size(size, size),
            cornerRadius = CornerRadius(3f * k),
            style = Stroke(width = k),
        )
    }

    // ---- Stamp --------------------------------------------------------------

    private fun DrawScope.drawStamp(
        a: Annotation,
        t: PageTransform,
        k: Float,
        textMeasurer: TextMeasurer,
        images: StampImageCache,
    ) {
        val r = urect(a, t) ?: return
        val boundUserRect =
            if (a.isBoundSignature()) {
                a.rect?.let { Rect(it.left, it.top, it.right, it.bottom) }
            } else {
                null
            }
        when (a.stampKind) {
            "image" -> {
                val key = a.imageSha256 ?: a.id
                val img = images.get(key, a.imageBase64) ?: return
                if (boundUserRect != null) {
                    val graphic = stampGraphicRect(a, boundUserRect, img.width.toFloat(), img.height.toFloat())
                    val origin = t.toContent(Offset(graphic.left, graphic.bottom))
                    val right = t.toContent(Offset(graphic.right, graphic.bottom))
                    val angle = atan2(right.y - origin.y, right.x - origin.x) * 180f / PI.toFloat()
                    withTransform({
                        translate(origin.x, origin.y)
                        rotate(angle, Offset.Zero)
                    }) {
                        drawImage(
                            image = img,
                            srcOffset = IntOffset.Zero,
                            srcSize = IntSize(img.width, img.height),
                            dstSize =
                                IntSize(
                                    (graphic.width * k).toInt().coerceAtLeast(1),
                                    (graphic.height * k).toInt().coerceAtLeast(1),
                                ),
                            alpha = a.opacity,
                        )
                    }
                    return
                }
                val (w, h) =
                    if (a.json["preserveAspect"]?.toString() != "false") {
                        val scale = min(r.width / img.width, r.height / img.height)
                        img.width * scale to img.height * scale
                    } else {
                        r.width to r.height
                    }
                drawImage(
                    image = img,
                    srcOffset = IntOffset.Zero,
                    srcSize = IntSize(img.width, img.height),
                    dstOffset =
                        IntOffset(
                            (r.left + (r.width - w) / 2f).toInt(),
                            (r.top + (r.height - h) / 2f).toInt(),
                        ),
                    dstSize = IntSize(w.toInt().coerceAtLeast(1), h.toInt().coerceAtLeast(1)),
                    alpha = a.opacity,
                )
            }
            "vector" -> {
                val vb =
                    a.json["vector"]?.let { v ->
                        v.jsonObject["viewBox"]?.jsonArray?.map { it.jsonPrimitive.content.toFloat() }
                    } ?: return
                if (vb.size < 4 || vb[2] <= 0f || vb[3] <= 0f) return
                val graphic = stampGraphicRect(a, boundUserRect ?: r, vb[2], vb[3])
                val sx = graphic.width / vb[2]
                val sy = graphic.height / vb[3]
                a.vectorPaths?.forEach { spec ->
                    val d = spec["d"]?.jsonPrimitive?.contentOrNull ?: return@forEach
                    val path =
                        parseVectorPath(d) { x, y ->
                            val point =
                                Offset(
                                    graphic.left + (x - vb[0]) * sx,
                                    graphic.top + (y - vb[1]) * sy,
                                )
                            if (boundUserRect != null) t.toContent(point) else point
                        } ?: return@forEach
                    spec["fill"]?.jsonPrimitive?.contentOrNull?.let {
                        drawPath(path, color = it.toComposeColor(a.opacity))
                    }
                    spec["stroke"]?.jsonPrimitive?.contentOrNull?.let {
                        val sw = spec["strokeWidth"]?.jsonPrimitive?.content?.toFloatOrNull() ?: 1f
                        drawPath(
                            path,
                            color = it.toComposeColor(a.opacity),
                            style = Stroke(width = sw * sx * if (boundUserRect != null) k else 1f),
                        )
                    }
                }
            }
            else -> {
                // "standard" (and fallback): red border + centred name.
                val graphic = stampGraphicRect(a, boundUserRect ?: r, 3f, 1f)
                if (boundUserRect == null) {
                    drawStandardStamp(a, graphic, k, textMeasurer)
                } else {
                    val origin = t.toContent(Offset(graphic.left, graphic.bottom))
                    val right = t.toContent(Offset(graphic.right, graphic.bottom))
                    val angle = atan2(right.y - origin.y, right.x - origin.x) * 180f / PI.toFloat()
                    withTransform({
                        translate(origin.x, origin.y)
                        rotate(angle, Offset.Zero)
                    }) {
                        drawStandardStamp(
                            a,
                            Rect(0f, 0f, graphic.width * k, graphic.height * k),
                            k,
                            textMeasurer,
                        )
                    }
                }
            }
        }
    }

    private fun DrawScope.drawStandardStamp(
        annotation: Annotation,
        graphic: Rect,
        k: Float,
        textMeasurer: TextMeasurer,
    ) {
        val bound = annotation.isBoundSignature()
        val name =
            (
                annotation.standardName
                    ?: if (bound) "STAMP" else annotation.stampKind ?: "STAMP"
            ).uppercase()
        drawRect(
            color = if (bound) Color.Red.copy(alpha = annotation.opacity) else Color.Red,
            topLeft = graphic.topLeft,
            size = graphic.size,
            style = Stroke(width = 2f * k),
        )
        if (bound) {
            val paint =
                Paint(Paint.ANTI_ALIAS_FLAG).apply {
                    color = android.graphics.Color.RED
                    alpha = (annotation.opacity.coerceIn(0f, 1f) * 255f).toInt()
                    typeface = Typeface.create("sans-serif", Typeface.BOLD)
                    textSize = graphic.height * 0.5f
                }
            while (paint.textSize > k && paint.measureText(name) > graphic.width - 4f * k) {
                paint.textSize -= 0.5f * k
            }
            paint.textSize = paint.textSize.coerceAtLeast(k)
            drawIntoCanvas {
                it.nativeCanvas.drawText(
                    name,
                    graphic.left + (graphic.width - paint.measureText(name)) / 2f,
                    graphic.top + graphic.height / 2f + 0.25f * paint.textSize,
                    paint,
                )
            }
            return
        }
        val style =
            TextStyle(
                fontSize = TextUnit(graphic.height * 0.5f / density, TextUnitType.Sp),
                color = Color.Red,
                textAlign = TextAlign.Center,
            )
        val laid =
            textMeasurer.measure(
                AnnotatedString(name),
                style = style,
                constraints = Constraints(maxWidth = graphic.width.toInt().coerceAtLeast(1)),
            )
        drawText(
            laid,
            topLeft =
                Offset(
                    graphic.left + (graphic.width - laid.size.width) / 2f,
                    graphic.top + (graphic.height - laid.size.height) / 2f,
                ),
        )
    }

    private fun stampGraphicRect(
        annotation: Annotation,
        target: Rect,
        width: Float,
        height: Float,
    ): Rect {
        if (!annotation.isBoundSignature() ||
            annotation.json["preserveAspect"]?.toString() == "false"
        ) {
            return target
        }
        val scale = min(target.width / width, target.height / height)
        val fittedWidth = width * scale
        val fittedHeight = height * scale
        val left = target.left + (target.width - fittedWidth) / 2f
        val top = target.top + (target.height - fittedHeight) / 2f
        return Rect(left, top, left + fittedWidth, top + fittedHeight)
    }

    private fun Annotation.isBoundSignature(): Boolean = json["bindToField"]?.jsonPrimitive?.contentOrNull != null

    /** Minimal `M L C Z` path parser; [map] converts viewBox coords to content px. */
    private fun parseVectorPath(
        d: String,
        map: (Float, Float) -> Offset,
    ): Path? {
        val tokens = d.split(Regex("[\\s,]+")).filter { it.isNotEmpty() }
        val path = Path()
        var i = 0
        var started = false

        fun num(): Float? = tokens.getOrNull(i)?.toFloatOrNull()?.also { i++ }
        while (i < tokens.size) {
            when (tokens[i++]) {
                "M" -> {
                    val x = num() ?: return null
                    val y = num() ?: return null
                    val p = map(x, y)
                    path.moveTo(p.x, p.y)
                    started = true
                }
                "L" -> {
                    val x = num() ?: return null
                    val y = num() ?: return null
                    val p = map(x, y)
                    path.lineTo(p.x, p.y)
                }
                "C" -> {
                    val nums = (0 until 6).map { num() ?: return null }
                    val c1 = map(nums[0], nums[1])
                    val c2 = map(nums[2], nums[3])
                    val e = map(nums[4], nums[5])
                    path.cubicTo(c1.x, c1.y, c2.x, c2.y, e.x, e.y)
                }
                "Z" -> path.close()
                else -> return null
            }
        }
        return if (started) path else null
    }

    // ---- Shapes --------------------------------------------------------------

    private fun DrawScope.drawRectShape(
        a: Annotation,
        t: PageTransform,
        k: Float,
        oval: Boolean,
    ) {
        val r = urect(a, t) ?: return
        a.fillColor?.let {
            if (oval) {
                drawOval(it.toComposeColor(a.opacity), topLeft = r.topLeft, size = r.size)
            } else {
                drawRect(it.toComposeColor(a.opacity), topLeft = r.topLeft, size = r.size)
            }
        }
        val stroke = Stroke(width = (a.strokeWidth ?: 2f) * k, pathEffect = dash(a))
        val color = (a.color ?: "#D32F2F").toComposeColor(a.opacity)
        if (oval) {
            drawOval(color, topLeft = r.topLeft, size = r.size, style = stroke)
        } else {
            drawRect(color, topLeft = r.topLeft, size = r.size, style = stroke)
        }
    }

    // ---- Line -----------------------------------------------------------------

    private fun DrawScope.drawLine(
        a: Annotation,
        t: PageTransform,
        k: Float,
    ) {
        val s = a.start ?: return
        val e = a.end ?: return
        val cs = upt(t, s.x, s.y)
        val ce = upt(t, e.x, e.y)
        val color = (a.color ?: "#000000").toComposeColor(a.opacity)
        val w = (a.strokeWidth ?: 1.5f) * k
        drawLine(color, start = cs, end = ce, strokeWidth = w)
        val endings = a.lineEndings ?: return
        val headLen = (4f * (a.strokeWidth ?: 1.5f) + 4f) * k
        val angle = atan2cs(ce - cs)
        endings.getOrNull(0)?.let { endingAt(cs, angle + PI.toFloat(), it, headLen, color, a, w) }
        endings.getOrNull(1)?.let { endingAt(ce, angle, it, headLen, color, a, w) }
    }

    private fun atan2cs(d: Offset): Float = kotlin.math.atan2(d.y, d.x)

    private fun DrawScope.endingAt(
        tip: Offset,
        dir: Float,
        kind: String,
        headLen: Float,
        color: Color,
        a: Annotation,
        w: Float,
    ) {
        val reversed = kind.startsWith("r")
        val base = kind.removePrefix("r")
        val d = if (reversed) dir + PI.toFloat() else dir
        when (base) {
            "openArrow", "closedArrow" -> {
                val half = 30f * PI.toFloat() / 180f
                val p1 = Offset(tip.x + headLen * cos(d - half), tip.y + headLen * sin(d - half))
                val p2 = Offset(tip.x + headLen * cos(d + half), tip.y + headLen * sin(d + half))
                if (base == "openArrow") {
                    drawLine(color, tip, p1, strokeWidth = w)
                    drawLine(color, tip, p2, strokeWidth = w)
                } else {
                    val path =
                        Path().apply {
                            moveTo(tip.x, tip.y)
                            lineTo(p1.x, p1.y)
                            lineTo(p2.x, p2.y)
                            close()
                        }
                    drawPath(path, color = (a.fillColor ?: a.color ?: "#000000").toComposeColor(a.opacity))
                }
            }
            "circle" -> drawCircle(color, radius = headLen / 2f, center = tip)
            "square" ->
                drawRect(
                    color,
                    topLeft = Offset(tip.x - headLen / 2f, tip.y - headLen / 2f),
                    size = Size(headLen, headLen),
                )
            else -> Unit
        }
    }

    private fun lerp(
        a: Float,
        b: Float,
        t: Float,
    ): Float = a + (b - a) * t.coerceIn(0f, 1f)
}
