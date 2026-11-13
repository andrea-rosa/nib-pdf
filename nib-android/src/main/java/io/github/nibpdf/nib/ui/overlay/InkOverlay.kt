package io.github.nibpdf.nib.ui.overlay

import android.util.Log
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.gestures.awaitEachGesture
import androidx.compose.foundation.gestures.awaitFirstDown
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.runtime.Composable
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.graphics.StrokeJoin
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.input.pointer.PointerType
import androidx.compose.ui.input.pointer.pointerInput
import io.github.nibpdf.nib.delta.DeltaIds
import io.github.nibpdf.nib.delta.DeltaOps
import io.github.nibpdf.nib.delta.InvalidDeltaException
import io.github.nibpdf.nib.delta.Rfc3339
import io.github.nibpdf.nib.ui.NibPdfViewState
import io.github.nibpdf.nib.ui.ink.InkSmoothing
import io.github.nibpdf.nib.ui.toComposeColor
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.buildJsonArray
import kotlinx.serialization.json.buildJsonObject
import kotlin.math.roundToLong

private const val RESAMPLE_SPACING_PT = 1.5f
private const val SIMPLIFY_TOLERANCE_PT = 0.3f

/**
 * Stylus/finger ink capture (ARCHITECTURE §5.2/§5.4). Samples are collected
 * in content px, drawn live as a cheap polyline, then mapped to user pt,
 * resampled + RDP-simplified and committed as one `create Ink` op on up.
 */
@Composable
@Suppress("ktlint:standard:function-naming")
internal fun InkOverlay(
    state: NibPdfViewState,
    page: Int,
) {
    val t = state.transform(page)
    val k = t.k
    val live = remember { mutableStateOf<List<InkSmoothing.Sample>>(emptyList()) }

    Box(
        Modifier
            .fillMaxSize()
            .pointerInput(page) {
                awaitEachGesture {
                    val down = awaitFirstDown()
                    val startedAtMs = down.uptimeMillis
                    val startedAtIso = Rfc3339.now()
                    val isStylus = down.type == PointerType.Stylus
                    val samples = mutableListOf<InkSmoothing.Sample>()
                    var lastUptime = -1L

                    fun record(
                        x: Float,
                        y: Float,
                        pressure: Float,
                        uptime: Long,
                    ) {
                        if (uptime <= lastUptime) return
                        lastUptime = uptime
                        samples += InkSmoothing.Sample(x, y, pressure, uptime - startedAtMs)
                    }

                    record(down.position.x, down.position.y, down.pressure, down.uptimeMillis)
                    live.value = samples.toList()

                    var change = down
                    while (change.pressed) {
                        val event = awaitPointerEvent()
                        change = event.changes.firstOrNull { it.id == down.id } ?: break
                        if (!change.pressed) break
                        for (h in change.historical) {
                            record(h.position.x, h.position.y, change.pressure, h.uptimeMillis)
                        }
                        record(change.position.x, change.position.y, change.pressure, change.uptimeMillis)
                        change.consume()
                        live.value = samples.toList()
                    }

                    if (samples.size >= 2) {
                        commit(state, page, samples, isStylus, startedAtIso)
                    }
                    live.value = emptyList()
                }
            },
    )

    Canvas(Modifier.fillMaxSize()) {
        val pts = live.value
        if (pts.size >= 2) {
            val path =
                Path().apply {
                    moveTo(pts[0].x, pts[0].y)
                    pts.drop(1).forEach { lineTo(it.x, it.y) }
                }
            drawPath(
                path,
                color = state.toolStyle.color.toComposeColor(state.toolStyle.opacity),
                style =
                    Stroke(
                        width = state.toolStyle.strokeWidth * k,
                        cap = StrokeCap.Round,
                        join = StrokeJoin.Round,
                    ),
            )
        }
    }
}

private fun commit(
    state: NibPdfViewState,
    page: Int,
    contentSamples: List<InkSmoothing.Sample>,
    isStylus: Boolean,
    startedAtIso: String,
) {
    val t = state.transform(page)
    val user =
        contentSamples.map { s ->
            val u = t.toUser(Offset(s.x, s.y))
            InkSmoothing.Sample(u.x, u.y, s.pressure, s.tMs)
        }
    val simplified = InkSmoothing.rdp(InkSmoothing.resample(user, RESAMPLE_SPACING_PT), SIMPLIFY_TOLERANCE_PT)
    if (simplified.size < 2) return

    fun r3(v: Float): Double = (v.toDouble() * 1000).roundToLong() / 1000.0

    fun r4(v: Float): Double = (v.toDouble() * 10000).roundToLong() / 10000.0

    val style = state.toolStyle
    val points =
        buildJsonArray {
            simplified.forEach { p ->
                add(
                    buildJsonArray {
                        add(JsonPrimitive(r3(p.x)))
                        add(JsonPrimitive(r3(p.y)))
                        add(JsonPrimitive(r4(p.pressure.coerceIn(0f, 1f))))
                        add(JsonPrimitive(p.tMs))
                    },
                )
            }
        }
    val payload =
        buildJsonObject {
            put("subtype", JsonPrimitive("Ink"))
            put("page", JsonPrimitive(page))
            put("color", JsonPrimitive(style.color))
            put("opacity", JsonPrimitive(style.opacity.toDouble()))
            put("strokeWidth", JsonPrimitive(style.strokeWidth.toDouble()))
            put("lineCap", JsonPrimitive("round"))
            put("lineJoin", JsonPrimitive("round"))
            put("pressureSensitive", JsonPrimitive(isStylus))
            put("minWidthFactor", JsonPrimitive(0.4))
            put("maxWidthFactor", JsonPrimitive(1.6))
            put(
                "smoothing",
                buildJsonObject {
                    put("algorithm", JsonPrimitive("catmullRom"))
                    put("tension", JsonPrimitive(0.5))
                    put("resampleSpacing", JsonPrimitive(RESAMPLE_SPACING_PT.toDouble()))
                    put("simplifyTolerance", JsonPrimitive(SIMPLIFY_TOLERANCE_PT.toDouble()))
                },
            )
            put(
                "strokes",
                buildJsonArray {
                    add(
                        buildJsonObject {
                            put("points", points)
                            put("tool", JsonPrimitive(if (isStylus) "stylus" else "finger"))
                            put("startedAt", JsonPrimitive(startedAtIso))
                        },
                    )
                },
            )
            state.author?.let { put("author", JsonPrimitive(it)) }
            put("createdAt", JsonPrimitive(Rfc3339.now()))
        }
    try {
        state.document.apply(DeltaOps.create(DeltaIds.uuidV4(), page, payload, state.author))
    } catch (e: InvalidDeltaException) {
        Log.w("nib", "ink create rejected: ${e.message}")
    }
}
