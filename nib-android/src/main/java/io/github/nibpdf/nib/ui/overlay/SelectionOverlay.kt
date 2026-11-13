package io.github.nibpdf.nib.ui.overlay

import android.util.Log
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.gestures.awaitEachGesture
import androidx.compose.foundation.gestures.awaitFirstDown
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.offset
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.PathEffect
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.text.rememberTextMeasurer
import androidx.compose.ui.unit.IntOffset
import io.github.nibpdf.nib.delta.Annotation
import io.github.nibpdf.nib.delta.DeltaOps
import io.github.nibpdf.nib.delta.InvalidDeltaException
import io.github.nibpdf.nib.ui.NibPdfViewState
import io.github.nibpdf.nib.ui.toComposeRect
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.buildJsonArray
import kotlinx.serialization.json.buildJsonObject
import kotlin.math.abs
import kotlin.math.max
import kotlin.math.min
import kotlin.math.roundToInt

private val HANDLE_PT = 8f
private val SELECTION_BLUE = Color(0xFF2962FF)

/**
 * Select / move / resize / delete for committed annotations
 * (ARCHITECTURE §5.2). Taps hit-test; drags on the bbox move, drags on a
 * corner handle resize; one `update` (or `delete`) op per gesture.
 */
@Composable
@Suppress("ktlint:standard:function-naming")
internal fun SelectionOverlay(
    state: NibPdfViewState,
    page: Int,
) {
    val version by state.document.annotationsVersion.collectAsState()
    val selected =
        remember(version, state.selectedId, page) {
            state.selectedId?.let { state.document.annotation(it) }?.takeIf { it.page == page }
        }
    val t = state.transform(page)
    val k = t.k
    val density = LocalDensity.current.density
    val textMeasurer = rememberTextMeasurer()
    val images = remember { StampImageCache() }

    // Live gesture preview in user space.
    val previewDelta = remember { mutableStateOf<Offset?>(null) }
    val previewRect = remember { mutableStateOf<Rect?>(null) }

    Box(
        Modifier
            .fillMaxSize()
            .pointerInput(page) {
                val handlePx = HANDLE_PT * density
                awaitEachGesture {
                    val down = awaitFirstDown()
                    val sel =
                        state.selectedId
                            ?.let { state.document.annotation(it) }
                            ?.takeIf { it.page == page }
                    val bboxC = sel?.rect?.let { t.rectToContent(it.toComposeRect()) }
                    var mode: Int = 0 // 0 none, 1 move, 2 resize-TL, 3 TR, 4 BR, 5 BL
                    if (sel != null && bboxC != null && !sel.flagsLocked) {
                        val corners =
                            listOf(
                                Offset(bboxC.left, bboxC.top),
                                Offset(bboxC.right, bboxC.top),
                                Offset(bboxC.right, bboxC.bottom),
                                Offset(bboxC.left, bboxC.bottom),
                            )
                        val hit =
                            corners.indexOfFirst { c ->
                                abs(down.position.x - c.x) <= handlePx && abs(down.position.y - c.y) <= handlePx
                            }
                        mode =
                            when {
                                hit >= 0 -> 2 + hit
                                bboxC.contains(down.position) -> 1
                                else -> 0
                            }
                    }
                    var dragging = false
                    var last = down.position
                    var change = down
                    while (change.pressed) {
                        val event = awaitPointerEvent()
                        change = event.changes.firstOrNull { it.id == down.id } ?: break
                        if (!change.pressed) break
                        val moved = change.position - last
                        if (!dragging &&
                            (change.position - down.position).getDistance() >
                            viewConfiguration.touchSlop
                        ) {
                            dragging = true
                        }
                        if (dragging && mode != 0 && sel != null && bboxC != null) {
                            val du = (moved.x / k)
                            val dv = (-moved.y / k)
                            if (mode == 1) {
                                previewDelta.value = (previewDelta.value ?: Offset.Zero) + Offset(du, dv)
                            } else {
                                previewRect.value =
                                    resizedUser(sel, t, mode, Offset(du, dv))
                            }
                            change.consume()
                        }
                        last = change.position
                    }
                    if (dragging && sel != null) {
                        val d = previewDelta.value
                        val r = previewRect.value
                        when {
                            d != null && mode == 1 -> submitMove(state, page, sel, d)
                            r != null && mode >= 2 -> submitResize(state, page, sel, r)
                        }
                    } else if (!dragging) {
                        val u = t.toUser(down.position)
                        val id =
                            state.document.hitTest(
                                page,
                                android.graphics.PointF(u.x, u.y),
                                6f / state.zoom.coerceAtLeast(0.25f),
                            )
                        state.selectedId = id
                    }
                    previewDelta.value = null
                    previewRect.value = null
                }
            },
    )

    Canvas(Modifier.fillMaxSize()) {
        if (selected != null) {
            val d = previewDelta.value
            val r = previewRect.value
            val bboxC =
                when {
                    r != null -> t.rectToContent(r)
                    else -> selected.rect?.let { t.rectToContent(it.toComposeRect()) }
                } ?: return@Canvas
            val box =
                if (d != null) {
                    bboxC.translate(d.x * k, -d.y * k)
                } else {
                    bboxC
                }
            if (d != null) {
                AnnotationDrawing.drawAnnotation(
                    this,
                    selected,
                    t,
                    k,
                    textMeasurer,
                    images,
                    offsetUser = d,
                )
            }
            drawRect(
                color = SELECTION_BLUE,
                topLeft = box.topLeft,
                size = box.size,
                style =
                    Stroke(
                        width = 1.5f * density,
                        pathEffect = PathEffect.dashPathEffect(floatArrayOf(6f * density, 4f * density)),
                    ),
            )
            val handlePx = HANDLE_PT * density
            listOf(
                Offset(box.left, box.top),
                Offset(box.right, box.top),
                Offset(box.right, box.bottom),
                Offset(box.left, box.bottom),
            ).forEach { c ->
                drawRect(
                    color = SELECTION_BLUE,
                    topLeft = Offset(c.x - handlePx / 2f, c.y - handlePx / 2f),
                    size = Size(handlePx, handlePx),
                )
            }
        }
    }

    if (selected != null && !selected.flagsLocked) {
        val bboxC = selected.rect?.let { t.rectToContent(it.toComposeRect()) }
        if (bboxC != null) {
            with(LocalDensity.current) {
                TextButton(
                    onClick = {
                        try {
                            state.document.apply(DeltaOps.delete(selected.id, page, state.author))
                        } catch (e: InvalidDeltaException) {
                            Log.w("nib", "delete rejected: ${e.message}")
                        }
                        state.selectedId = null
                    },
                    modifier =
                        Modifier
                            .offset {
                                IntOffset(
                                    bboxC.left.roundToInt(),
                                    (bboxC.top - 36f * density).roundToInt().coerceAtLeast(0),
                                )
                            }.testTag("deleteSelected"),
                ) { Text("Delete") }
            }
        }
    }
}

/** Anchor the opposite corner of [sel]'s rect and move corner [mode] (2 TL, 3 TR, 4 BR, 5 BL) by [dUser]. */
private fun resizedUser(
    sel: Annotation,
    t: io.github.nibpdf.nib.ui.PageTransform,
    mode: Int,
    dUser: Offset,
): Rect? {
    val r = sel.rect ?: return null
    var (x0, y0, x1, y1) = listOf(r.left, r.top, r.right, r.bottom)
    when (mode) {
        2 -> {
            x0 += dUser.x
            y0 += dUser.y
        }
        3 -> {
            x1 += dUser.x
            y0 += dUser.y
        }
        4 -> {
            x1 += dUser.x
            y1 += dUser.y
        }
        5 -> {
            x0 += dUser.x
            y1 += dUser.y
        }
    }
    val nx0 = min(x0, x1)
    val ny0 = min(y0, y1)
    val nx1 = max(x0, x1)
    val ny1 = max(y0, y1)
    if (nx1 - nx0 < 4f || ny1 - ny0 < 4f) return null
    return Rect(nx0, ny0, nx1, ny1)
}

private fun submitMove(
    state: NibPdfViewState,
    page: Int,
    sel: Annotation,
    d: Offset,
) {
    val patch =
        buildJsonObject {
            sel.rect?.let { r ->
                put(
                    "rect",
                    buildJsonArray {
                        listOf(r.left + d.x, r.top + d.y, r.right + d.x, r.bottom + d.y)
                            .forEach { add(JsonPrimitive(it.toDouble())) }
                    },
                )
            }
            sel.strokes?.let { strokes ->
                put(
                    "strokes",
                    buildJsonArray {
                        strokes.forEach { stroke ->
                            add(
                                buildJsonObject {
                                    put(
                                        "points",
                                        buildJsonArray {
                                            stroke.forEach { p ->
                                                add(
                                                    buildJsonArray {
                                                        add(JsonPrimitive((p[0] + d.x).toDouble()))
                                                        add(JsonPrimitive((p[1] + d.y).toDouble()))
                                                        for (i in 2 until p.size) add(JsonPrimitive(p[i].toDouble()))
                                                    },
                                                )
                                            }
                                        },
                                    )
                                },
                            )
                        }
                    },
                )
            }
            sel.quadPoints?.let { quads ->
                put(
                    "quadPoints",
                    buildJsonArray {
                        quads.forEach { q ->
                            add(
                                buildJsonArray {
                                    for (i in q.indices step 2) {
                                        add(JsonPrimitive((q[i] + d.x).toDouble()))
                                        add(JsonPrimitive((q[i + 1] + d.y).toDouble()))
                                    }
                                },
                            )
                        }
                    },
                )
            }
            sel.start?.let { s ->
                put(
                    "start",
                    buildJsonArray {
                        add(JsonPrimitive((s.x + d.x).toDouble()))
                        add(JsonPrimitive((s.y + d.y).toDouble()))
                    },
                )
            }
            sel.end?.let { e ->
                put(
                    "end",
                    buildJsonArray {
                        add(JsonPrimitive((e.x + d.x).toDouble()))
                        add(JsonPrimitive((e.y + d.y).toDouble()))
                    },
                )
            }
        }
    try {
        state.document.apply(DeltaOps.update(sel.id, patch, page, state.author))
    } catch (e: InvalidDeltaException) {
        Log.w("nib", "move rejected: ${e.message}")
    }
}

private fun submitResize(
    state: NibPdfViewState,
    page: Int,
    sel: Annotation,
    newRect: Rect,
) {
    val old = sel.rect ?: return
    val sx = if (old.right - old.left > 0f) (newRect.width) / (old.right - old.left) else 1f
    val sy = if (old.bottom - old.top > 0f) (newRect.height) / (old.bottom - old.top) else 1f

    fun scalePt(
        x: Float,
        y: Float,
    ): Pair<Float, Float> =
        Pair(
            newRect.left + (x - old.left) * sx,
            newRect.top + (y - old.top) * sy,
        )

    val patch =
        buildJsonObject {
            put(
                "rect",
                buildJsonArray {
                    listOf(newRect.left, newRect.top, newRect.right, newRect.bottom)
                        .forEach { add(JsonPrimitive(it.toDouble())) }
                },
            )
            sel.strokes?.let { strokes ->
                put(
                    "strokes",
                    buildJsonArray {
                        strokes.forEach { stroke ->
                            add(
                                buildJsonObject {
                                    put(
                                        "points",
                                        buildJsonArray {
                                            stroke.forEach { p ->
                                                val (x, y) = scalePt(p[0], p[1])
                                                add(
                                                    buildJsonArray {
                                                        add(JsonPrimitive(x.toDouble()))
                                                        add(JsonPrimitive(y.toDouble()))
                                                        for (i in 2 until p.size) add(JsonPrimitive(p[i].toDouble()))
                                                    },
                                                )
                                            }
                                        },
                                    )
                                },
                            )
                        }
                    },
                )
            }
            sel.quadPoints?.let { quads ->
                put(
                    "quadPoints",
                    buildJsonArray {
                        quads.forEach { q ->
                            add(
                                buildJsonArray {
                                    for (i in q.indices step 2) {
                                        val (x, y) = scalePt(q[i], q[i + 1])
                                        add(JsonPrimitive(x.toDouble()))
                                        add(JsonPrimitive(y.toDouble()))
                                    }
                                },
                            )
                        }
                    },
                )
            }
            sel.start?.let { s ->
                val (x, y) = scalePt(s.x, s.y)
                put(
                    "start",
                    buildJsonArray {
                        add(JsonPrimitive(x.toDouble()))
                        add(JsonPrimitive(y.toDouble()))
                    },
                )
            }
            sel.end?.let { e ->
                val (x, y) = scalePt(e.x, e.y)
                put(
                    "end",
                    buildJsonArray {
                        add(JsonPrimitive(x.toDouble()))
                        add(JsonPrimitive(y.toDouble()))
                    },
                )
            }
        }
    try {
        state.document.apply(DeltaOps.update(sel.id, patch, page, state.author))
    } catch (e: InvalidDeltaException) {
        Log.w("nib", "resize rejected: ${e.message}")
    }
}
