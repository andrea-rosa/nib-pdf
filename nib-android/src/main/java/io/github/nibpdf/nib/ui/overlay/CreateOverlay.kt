package io.github.nibpdf.nib.ui.overlay

import android.util.Base64
import android.util.Log
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.gestures.detectDragGestures
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.runtime.Composable
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.graphics.PathEffect
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.input.pointer.pointerInput
import io.github.nibpdf.nib.delta.DeltaIds
import io.github.nibpdf.nib.delta.DeltaOps
import io.github.nibpdf.nib.delta.InvalidDeltaException
import io.github.nibpdf.nib.delta.Rfc3339
import io.github.nibpdf.nib.ui.NibPdfViewState
import io.github.nibpdf.nib.ui.StampSpec
import io.github.nibpdf.nib.ui.Tool
import io.github.nibpdf.nib.ui.toComposeColor
import kotlinx.coroutines.launch
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.buildJsonArray
import kotlinx.serialization.json.buildJsonObject
import java.security.MessageDigest
import kotlin.math.abs
import kotlin.math.max
import kotlin.math.min

private val DRAG_TOOLS =
    setOf(
        Tool.Highlight,
        Tool.Underline,
        Tool.StrikeOut,
        Tool.Square,
        Tool.Circle,
        Tool.Line,
    )

/**
 * Creation gestures for markup, shapes, FreeText, Note and Stamp
 * (ARCHITECTURE §5.2). Preview in content px; ops carry user-space coords.
 */
@Composable
@Suppress("ktlint:standard:function-naming")
internal fun CreateOverlay(
    state: NibPdfViewState,
    page: Int,
) {
    val scope = rememberCoroutineScope()
    val dragStart = remember { mutableStateOf<Offset?>(null) }
    val dragEnd = remember { mutableStateOf<Offset?>(null) }
    val tool = state.tool

    Box(
        Modifier
            .fillMaxSize()
            .pointerInput(tool, page) {
                if (tool in DRAG_TOOLS) {
                    detectDragGestures(
                        onDragStart = { pos ->
                            dragStart.value = pos
                            dragEnd.value = pos
                        },
                        onDrag = { change, dragAmount ->
                            dragEnd.value = (dragEnd.value ?: change.position) + dragAmount
                            change.consume()
                        },
                        onDragEnd = {
                            val a = dragStart.value
                            val b = dragEnd.value
                            dragStart.value = null
                            dragEnd.value = null
                            if (a != null && b != null) onDragEnd(state, page, tool, a, b, scope)
                        },
                        onDragCancel = {
                            dragStart.value = null
                            dragEnd.value = null
                        },
                    )
                } else {
                    detectTapGestures { pos -> onTap(state, page, tool, pos, scope) }
                }
            },
    )

    Canvas(Modifier.fillMaxSize()) {
        val a = dragStart.value
        val b = dragEnd.value
        if (a != null && b != null) {
            val color = state.toolStyle.color.toComposeColor()
            if (tool == Tool.Line) {
                drawLine(
                    color = color,
                    start = a,
                    end = b,
                    strokeWidth = state.toolStyle.strokeWidth * t(state, page),
                )
            } else {
                val r = normRect(a, b)
                drawRect(
                    color = color.copy(alpha = 0.15f),
                    topLeft = r.topLeft,
                    size = r.size,
                )
                drawRect(
                    color = color,
                    topLeft = r.topLeft,
                    size = r.size,
                    style = Stroke(width = 2f, pathEffect = PathEffect.dashPathEffect(floatArrayOf(8f, 6f))),
                )
            }
        }
    }
}

private fun t(
    state: NibPdfViewState,
    page: Int,
): Float = state.transform(page).k

private fun normRect(
    a: Offset,
    b: Offset,
): Rect = Rect(min(a.x, b.x), min(a.y, b.y), max(a.x, b.x), max(a.y, b.y))

private fun onDragEnd(
    state: NibPdfViewState,
    page: Int,
    tool: Tool,
    startContent: Offset,
    endContent: Offset,
    scope: kotlinx.coroutines.CoroutineScope,
) {
    val t = state.transform(page)
    val sa = t.toUser(startContent)
    val sb = t.toUser(endContent)
    val style = state.toolStyle
    val userRect = Rect(min(sa.x, sb.x), min(sa.y, sb.y), max(sa.x, sb.y), max(sa.y, sb.y))
    val wPt = abs(sb.x - sa.x)
    val hPt = abs(sb.y - sa.y)

    fun submit(payload: kotlinx.serialization.json.JsonObject) {
        try {
            state.document.apply(DeltaOps.create(DeltaIds.uuidV4(), page, payload, state.author))
        } catch (e: InvalidDeltaException) {
            Log.w("nib", "create rejected: ${e.message}")
        }
    }

    when (tool) {
        Tool.Highlight, Tool.Underline, Tool.StrikeOut -> {
            scope.launch {
                val rectF = android.graphics.RectF(userRect.left, userRect.top, userRect.right, userRect.bottom)
                val lines = state.document.textLinesInRect(page, rectF)
                if (lines.isEmpty()) return@launch
                val quads =
                    buildJsonArray {
                        lines.forEach { r ->
                            // UL, UR, LL, LR (y-up).
                            add(
                                buildJsonArray {
                                    listOf(r.left, r.bottom, r.right, r.bottom, r.left, r.top, r.right, r.top)
                                        .forEach { add(JsonPrimitive(it.toDouble())) }
                                },
                            )
                        }
                    }
                val subtype =
                    when (tool) {
                        Tool.Highlight -> "Highlight"
                        Tool.Underline -> "Underline"
                        else -> "StrikeOut"
                    }
                val selectedText = state.document.textInRect(page, rectF)
                val payload =
                    buildJsonObject {
                        put("subtype", JsonPrimitive(subtype))
                        put("page", JsonPrimitive(page))
                        put(
                            "color",
                            JsonPrimitive(if (tool == Tool.Highlight) "#FFEB3B" else style.color),
                        )
                        put(
                            "opacity",
                            JsonPrimitive((if (tool == Tool.Highlight) 0.5f else style.opacity).toDouble()),
                        )
                        if (tool == Tool.Highlight) put("blendMode", JsonPrimitive("multiply"))
                        put("quadPoints", quads)
                        put("selectedText", JsonPrimitive(selectedText))
                        state.author?.let { put("author", JsonPrimitive(it)) }
                        put("createdAt", JsonPrimitive(Rfc3339.now()))
                    }
                submit(payload)
            }
        }
        Tool.Square, Tool.Circle -> {
            if (wPt < 2f || hPt < 2f) return
            val payload =
                buildJsonObject {
                    put("subtype", JsonPrimitive(if (tool == Tool.Square) "Square" else "Circle"))
                    put("page", JsonPrimitive(page))
                    put(
                        "rect",
                        buildJsonArray {
                            listOf(userRect.left, userRect.top, userRect.right, userRect.bottom)
                                .forEach { add(JsonPrimitive(it.toDouble())) }
                        },
                    )
                    put("color", JsonPrimitive(style.color))
                    style.fillColor?.let { put("fillColor", JsonPrimitive(it)) }
                    put("opacity", JsonPrimitive(style.opacity.toDouble()))
                    put("strokeWidth", JsonPrimitive(style.strokeWidth.toDouble()))
                    put(
                        "borderStyle",
                        buildJsonObject { put("style", JsonPrimitive("solid")) },
                    )
                    state.author?.let { put("author", JsonPrimitive(it)) }
                    put("createdAt", JsonPrimitive(Rfc3339.now()))
                }
            submit(payload)
        }
        Tool.Line -> {
            if (wPt < 2f && hPt < 2f) return
            val payload =
                buildJsonObject {
                    put("subtype", JsonPrimitive("Line"))
                    put("page", JsonPrimitive(page))
                    put(
                        "start",
                        buildJsonArray {
                            add(JsonPrimitive(sa.x.toDouble()))
                            add(JsonPrimitive(sa.y.toDouble()))
                        },
                    )
                    put(
                        "end",
                        buildJsonArray {
                            add(JsonPrimitive(sb.x.toDouble()))
                            add(JsonPrimitive(sb.y.toDouble()))
                        },
                    )
                    put("color", JsonPrimitive(style.color))
                    put("strokeWidth", JsonPrimitive(style.strokeWidth.toDouble()))
                    put(
                        "lineEndings",
                        buildJsonArray {
                            add(JsonPrimitive("none"))
                            add(JsonPrimitive("none"))
                        },
                    )
                    state.author?.let { put("author", JsonPrimitive(it)) }
                    put("createdAt", JsonPrimitive(Rfc3339.now()))
                }
            submit(payload)
        }
        else -> Unit
    }
}

private fun onTap(
    state: NibPdfViewState,
    page: Int,
    tool: Tool,
    posContent: Offset,
    scope: kotlinx.coroutines.CoroutineScope,
) {
    val t = state.transform(page)
    val tap = t.toUser(posContent)
    val style = state.toolStyle
    val crop = state.document.pages[page].cropBox

    fun submit(payload: kotlinx.serialization.json.JsonObject) {
        try {
            state.document.apply(DeltaOps.create(DeltaIds.uuidV4(), page, payload, state.author))
        } catch (e: InvalidDeltaException) {
            Log.w("nib", "create rejected: ${e.message}")
        }
    }

    fun clampRect(
        w: Float,
        h: Float,
        centerX: Boolean = false,
    ): Rect {
        var x = if (centerX) tap.x - w / 2f else tap.x
        var y = tap.y
        x = x.coerceIn(crop.left, (crop.right - w).coerceAtLeast(crop.left))
        y = y.coerceIn(crop.top, (crop.bottom - h).coerceAtLeast(crop.top))
        return Rect(x, y, x + w, y + h)
    }

    fun rectArray(r: Rect) =
        buildJsonArray {
            listOf(r.left, r.top, r.right, r.bottom).forEach { add(JsonPrimitive(it.toDouble())) }
        }

    when (tool) {
        Tool.FreeText -> {
            scope.launch {
                val text = state.onRequestText?.invoke("") ?: "Text"
                if (text.isNullOrBlank()) return@launch
                val r = clampRect(200f, 60f)
                val payload =
                    buildJsonObject {
                        put("subtype", JsonPrimitive("FreeText"))
                        put("page", JsonPrimitive(page))
                        put("rect", rectArray(r))
                        put("contents", JsonPrimitive(text))
                        put("color", JsonPrimitive(style.color))
                        put("fontFamily", JsonPrimitive("Helvetica"))
                        put("fontSize", JsonPrimitive(style.fontSize.toDouble()))
                        put("textAlign", JsonPrimitive("left"))
                        put("verticalAlign", JsonPrimitive("top"))
                        put("lineHeight", JsonPrimitive(1.2))
                        put("padding", JsonPrimitive(2.0))
                        put("fillColor", JsonPrimitive("#FFFFFF"))
                        put("borderColor", JsonPrimitive(style.color))
                        put("borderWidth", JsonPrimitive(1.0))
                        put("borderStyle", buildJsonObject { put("style", JsonPrimitive("solid")) })
                        state.author?.let { put("author", JsonPrimitive(it)) }
                        put("createdAt", JsonPrimitive(Rfc3339.now()))
                    }
                submit(payload)
            }
        }
        Tool.Note -> {
            val r = clampRect(20f, 20f)
            val payload =
                buildJsonObject {
                    put("subtype", JsonPrimitive("Text"))
                    put("page", JsonPrimitive(page))
                    put("rect", rectArray(r))
                    put("color", JsonPrimitive("#FFC107"))
                    put("icon", JsonPrimitive("comment"))
                    put("open", JsonPrimitive(false))
                    state.author?.let { put("author", JsonPrimitive(it)) }
                    put("createdAt", JsonPrimitive(Rfc3339.now()))
                }
            submit(payload)
        }
        Tool.Stamp -> {
            when (val stamp = state.pendingStamp) {
                is StampSpec.Image -> {
                    val sha =
                        MessageDigest
                            .getInstance("SHA-256")
                            .digest(stamp.png)
                            .joinToString("") { "%02x".format(it) }
                    val wPt = 120f
                    val hPt = if (stamp.widthPx > 0) wPt * stamp.heightPx / stamp.widthPx else 120f
                    val r = clampRect(wPt, hPt, centerX = true)
                    val payload =
                        buildJsonObject {
                            put("subtype", JsonPrimitive("Stamp"))
                            put("page", JsonPrimitive(page))
                            put("rect", rectArray(r))
                            put("opacity", JsonPrimitive(style.opacity.toDouble()))
                            put("rotation", JsonPrimitive(0.0))
                            put("kind", JsonPrimitive("image"))
                            put(
                                "image",
                                buildJsonObject {
                                    put("mime", JsonPrimitive(stamp.mime))
                                    put("sha256", JsonPrimitive("sha256:$sha"))
                                    put("widthPx", JsonPrimitive(stamp.widthPx))
                                    put("heightPx", JsonPrimitive(stamp.heightPx))
                                    put(
                                        "dataBase64",
                                        JsonPrimitive(Base64.encodeToString(stamp.png, Base64.NO_WRAP)),
                                    )
                                },
                            )
                            put("preserveAspect", JsonPrimitive(true))
                            state.author?.let { put("author", JsonPrimitive(it)) }
                            put("createdAt", JsonPrimitive(Rfc3339.now()))
                        }
                    submit(payload)
                }
                is StampSpec.Standard -> {
                    val r = clampRect(120f, 40f, centerX = true)
                    val payload =
                        buildJsonObject {
                            put("subtype", JsonPrimitive("Stamp"))
                            put("page", JsonPrimitive(page))
                            put("rect", rectArray(r))
                            put("opacity", JsonPrimitive(style.opacity.toDouble()))
                            put("rotation", JsonPrimitive(0.0))
                            put("kind", JsonPrimitive("standard"))
                            put("standardName", JsonPrimitive(stamp.name))
                            put("preserveAspect", JsonPrimitive(true))
                            state.author?.let { put("author", JsonPrimitive(it)) }
                            put("createdAt", JsonPrimitive(Rfc3339.now()))
                        }
                    submit(payload)
                }
                null -> Unit
            }
        }
        else -> Unit
    }
}
