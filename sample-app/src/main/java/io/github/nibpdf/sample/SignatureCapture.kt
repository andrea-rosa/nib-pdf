package io.github.nibpdf.sample

import android.graphics.Bitmap
import android.graphics.Paint
import android.graphics.Typeface
import android.util.Base64
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.gestures.awaitEachGesture
import androidx.compose.foundation.gestures.awaitFirstDown
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateListOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.unit.dp
import io.github.nibpdf.nib.delta.Rfc3339
import io.github.nibpdf.nib.ui.VisualSignatureRequest
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.add
import kotlinx.serialization.json.buildJsonArray
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.put
import kotlin.math.ceil

internal sealed interface SignatureMark {
    data class Ink(
        val strokes: List<List<Offset>>,
        val strokeWidth: Float,
    ) : SignatureMark

    data class Image(
        val png: ByteArray,
        val width: Int,
        val height: Int,
    ) : SignatureMark
}

internal data class CapturedSignature(
    val mark: SignatureMark,
    val signerName: String?,
)

private enum class CaptureMode { Draw, Type, Image }

@Composable
@Suppress("ktlint:standard:function-naming")
internal fun SignatureCaptureDialog(
    previous: CapturedSignature?,
    pickedImage: Bitmap?,
    onPickImage: () -> Unit,
    onDismiss: () -> Unit,
    onApply: (CapturedSignature) -> Unit,
    onCaptureError: (String) -> Unit,
) {
    var mode by remember { mutableStateOf(CaptureMode.Draw) }
    var signerName by remember { mutableStateOf("") }
    var typedText by remember { mutableStateOf("") }
    val strokes = remember { mutableStateListOf<List<Offset>>() }
    val strokeWidth = with(LocalDensity.current) { 3.dp.toPx() }
    val canApply =
        when (mode) {
            CaptureMode.Draw -> strokes.isNotEmpty()
            CaptureMode.Type -> typedText.isNotBlank()
            CaptureMode.Image -> pickedImage != null
        }

    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text("Visual signature") },
        text = {
            Column(
                Modifier.heightIn(max = 440.dp).verticalScroll(rememberScrollState()),
                verticalArrangement = Arrangement.spacedBy(8.dp),
            ) {
                Text("This is a visual mark, not a cryptographic signature.")
                Row(horizontalArrangement = Arrangement.spacedBy(4.dp)) {
                    CaptureMode.entries.forEach { option ->
                        TextButton(
                            onClick = { mode = option },
                            modifier = Modifier.testTag("signature:mode:${option.name.lowercase()}"),
                        ) {
                            Text(option.name + if (mode == option) " ✓" else "")
                        }
                    }
                }
                OutlinedTextField(
                    value = signerName,
                    onValueChange = { signerName = it.take(120) },
                    label = { Text("Signer name (optional)") },
                    singleLine = true,
                    modifier = Modifier.fillMaxWidth().testTag("signature:signer"),
                )
                when (mode) {
                    CaptureMode.Draw -> {
                        Canvas(
                            Modifier
                                .fillMaxWidth()
                                .height(150.dp)
                                .background(Color.White)
                                .border(1.dp, Color.Gray)
                                .testTag("signature:canvas")
                                .pointerInput(Unit) {
                                    awaitEachGesture {
                                        val down = awaitFirstDown(requireUnconsumed = false)
                                        val points = mutableListOf(down.position)
                                        val index = strokes.size
                                        strokes.add(points.toList())
                                        down.consume()
                                        do {
                                            val event = awaitPointerEvent()
                                            val change = event.changes.firstOrNull { it.id == down.id } ?: break
                                            if (change.pressed && change.position != points.last()) {
                                                points.add(change.position)
                                                strokes[index] = points.toList()
                                            }
                                            change.consume()
                                        } while (change.pressed)
                                    }
                                },
                        ) {
                            strokes.forEach { points ->
                                if (points.size == 1) {
                                    drawCircle(Color(0xFF1A237E), strokeWidth / 2f, points.first())
                                } else {
                                    val path =
                                        Path().apply {
                                            moveTo(points.first().x, points.first().y)
                                            points.drop(1).forEach { lineTo(it.x, it.y) }
                                        }
                                    drawPath(path, Color(0xFF1A237E), style = Stroke(strokeWidth, cap = StrokeCap.Round))
                                }
                            }
                        }
                        TextButton(
                            onClick = { strokes.clear() },
                            modifier = Modifier.testTag("signature:clear"),
                        ) { Text("Clear") }
                    }
                    CaptureMode.Type ->
                        OutlinedTextField(
                            value = typedText,
                            onValueChange = { typedText = it.take(120) },
                            label = { Text("Signature text") },
                            singleLine = true,
                            modifier = Modifier.fillMaxWidth().testTag("signature:text"),
                        )
                    CaptureMode.Image -> {
                        TextButton(
                            onClick = onPickImage,
                            modifier = Modifier.testTag("signature:pickImage"),
                        ) { Text("Choose image") }
                        pickedImage?.let { bitmap ->
                            Image(
                                bitmap = bitmap.asImageBitmap(),
                                contentDescription = "Selected visual signature image",
                                contentScale = ContentScale.Fit,
                                modifier = Modifier.fillMaxWidth().height(100.dp).testTag("signature:imagePreview"),
                            )
                        }
                    }
                }
                if (previous != null) {
                    TextButton(
                        onClick = { onApply(previous) },
                        modifier = Modifier.testTag("signature:reuse"),
                    ) { Text("Reuse last mark") }
                }
            }
        },
        confirmButton = {
            Button(
                enabled = canApply,
                onClick = capture@{
                    val mark =
                        try {
                            when (mode) {
                                CaptureMode.Draw -> SignatureMark.Ink(strokes.map { it.toList() }, strokeWidth)
                                CaptureMode.Type -> typedSignature(typedText.trim())
                                CaptureMode.Image -> {
                                    val bitmap = requireNotNull(pickedImage)
                                    SignatureMark.Image(bitmap.toPng(), bitmap.width, bitmap.height)
                                }
                            }
                        } catch (e: IllegalArgumentException) {
                            onCaptureError(e.message ?: "Invalid signature image")
                            return@capture
                        } catch (e: IllegalStateException) {
                            onCaptureError(e.message ?: "Could not encode signature image")
                            return@capture
                        }
                    val name = signerName.trim().ifEmpty { if (mode == CaptureMode.Type) typedText.trim() else "" }
                    onApply(CapturedSignature(mark, name.ifEmpty { null }))
                },
                modifier = Modifier.testTag("signature:apply"),
            ) { Text("Add mark") }
        },
        dismissButton = {
            TextButton(
                onClick = onDismiss,
                modifier = Modifier.testTag("signature:cancel"),
            ) { Text("Cancel") }
        },
    )
}

private fun typedSignature(text: String): SignatureMark.Image {
    val paint =
        Paint(Paint.ANTI_ALIAS_FLAG).apply {
            color = android.graphics.Color.rgb(26, 35, 126)
            textSize = 64f
            typeface = Typeface.create("cursive", Typeface.NORMAL)
        }
    val measured = paint.measureText(text)
    if (measured > 2000f) paint.textSize *= 2000f / measured
    val metrics = paint.fontMetrics
    val width = ceil(paint.measureText(text) + 32f).toInt().coerceAtLeast(1)
    val height = ceil(metrics.descent - metrics.ascent + 32f).toInt().coerceAtLeast(1)
    val bitmap = Bitmap.createBitmap(width, height, Bitmap.Config.ARGB_8888)
    android.graphics.Canvas(bitmap).drawText(text, 16f, 16f - metrics.ascent, paint)
    return SignatureMark.Image(bitmap.toPng(), width, height)
}

internal fun signaturePayload(
    request: VisualSignatureRequest,
    captured: CapturedSignature,
): JsonObject =
    buildJsonObject {
        put("page", request.page)
        put("intent", "signature")
        put("bindToField", request.fieldName)
        request.widgetId?.let { put("bindToWidget", it) }
        put("fitMode", "contain")
        put(
            "signer",
            buildJsonObject {
                captured.signerName?.let { put("name", it) }
                put("signedAt", Rfc3339.now())
            },
        )
        when (val mark = captured.mark) {
            is SignatureMark.Ink -> {
                put("subtype", "Ink")
                put("color", "#1A237E")
                put("strokeWidth", mark.strokeWidth)
                put(
                    "strokes",
                    buildJsonArray {
                        mark.strokes.forEach { stroke ->
                            add(
                                buildJsonObject {
                                    put(
                                        "points",
                                        buildJsonArray {
                                            stroke.forEach { point ->
                                                add(
                                                    buildJsonArray {
                                                        add(point.x)
                                                        add(-point.y)
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
            is SignatureMark.Image -> {
                put("subtype", "Stamp")
                put("kind", "image")
                put(
                    "rect",
                    buildJsonArray {
                        add(request.rect.left)
                        add(request.rect.top)
                        add(request.rect.right)
                        add(request.rect.bottom)
                    },
                )
                put(
                    "image",
                    buildJsonObject {
                        put("mime", "image/png")
                        put("widthPx", mark.width)
                        put("heightPx", mark.height)
                        put("dataBase64", Base64.encodeToString(mark.png, Base64.NO_WRAP))
                    },
                )
            }
        }
    }
