package io.github.nibpdf.sample

import android.graphics.BitmapFactory
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material3.Button
import androidx.compose.material3.FilterChip
import androidx.compose.material3.Slider
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.unit.dp
import io.github.nibpdf.nib.ui.NibPdfViewState
import io.github.nibpdf.nib.ui.Tool

private val SWATCHES = listOf("#1A237E", "#D32F2F", "#FFEB3B", "#388E3C", "#000000")

/**
 * Two-row bottom bar: tool chips, then undo/redo, colour swatches,
 * stroke-width slider, stamp buttons and delta export.
 */
@Composable
@Suppress("ktlint:standard:function-naming")
internal fun ToolPalette(
    state: NibPdfViewState,
    bundledStamp: android.graphics.Bitmap?,
    saveEnabled: Boolean,
    onSave: () -> Unit,
    onSaveAs: () -> Unit,
    onPickStamp: () -> Unit,
    onExport: () -> Unit,
) {
    Column(Modifier.fillMaxWidth().padding(horizontal = 8.dp, vertical = 4.dp)) {
        Row(
            Modifier.fillMaxWidth().horizontalScroll(rememberScrollState()),
            horizontalArrangement = Arrangement.spacedBy(6.dp),
        ) {
            Tool.entries.forEach { tool ->
                FilterChip(
                    selected = state.tool == tool,
                    onClick = { state.tool = tool },
                    label = { Text(tool.name) },
                    modifier = Modifier.testTag("tool:${tool.name}"),
                )
            }
        }
        Row(
            Modifier
                .fillMaxWidth()
                .horizontalScroll(rememberScrollState())
                .padding(top = 4.dp),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(6.dp),
        ) {
            val undoAvailable by state.document.undoAvailable.collectAsState()
            val redoAvailable by state.document.redoAvailable.collectAsState()
            TextButton(
                onClick = { state.document.undo() },
                enabled = undoAvailable,
                modifier = Modifier.testTag("undo"),
            ) { Text("Undo") }
            TextButton(
                onClick = { state.document.redo() },
                enabled = redoAvailable,
                modifier = Modifier.testTag("redo"),
            ) { Text("Redo") }
            SWATCHES.forEach { hex ->
                val c = Color(android.graphics.Color.parseColor(hex))
                val selected = state.toolStyle.color.equals(hex, ignoreCase = true)
                Spacer(
                    Modifier
                        .size(22.dp)
                        .background(c, CircleShape)
                        .border(
                            width = if (selected) 3.dp else 1.dp,
                            color = if (selected) Color.Black else Color.Gray,
                            shape = CircleShape,
                        ).clickable { state.toolStyle = state.toolStyle.copy(color = hex) }
                        .testTag("swatch:$hex"),
                )
            }
            Slider(
                value = state.toolStyle.strokeWidth,
                onValueChange = { state.toolStyle = state.toolStyle.copy(strokeWidth = it) },
                valueRange = 1f..8f,
                modifier = Modifier.width(80.dp).testTag("strokeWidth"),
            )
            TextButton(
                onClick = {
                    bundledStamp?.let { bmp ->
                        state.pendingStamp =
                            io.github.nibpdf.nib.ui.StampSpec.Image(
                                png = bmp.toPng(),
                                widthPx = bmp.width,
                                heightPx = bmp.height,
                            )
                        state.tool = Tool.Stamp
                    }
                },
                modifier = Modifier.testTag("stampBundled"),
            ) { Text("Stamp") }
            TextButton(
                onClick = onPickStamp,
                modifier = Modifier.testTag("stampPick"),
            ) { Text("Stamp…") }
            Button(onClick = onExport, modifier = Modifier.testTag("export")) { Text("Export") }
            Button(
                onClick = onSave,
                enabled = saveEnabled,
                modifier = Modifier.testTag("action:save"),
            ) { Text("Save") }
            Button(onClick = onSaveAs, modifier = Modifier.testTag("action:saveAs")) { Text("Save As…") }
        }
    }
}

internal fun android.graphics.Bitmap.toPng(): ByteArray {
    val out = java.io.ByteArrayOutputStream()
    compress(android.graphics.Bitmap.CompressFormat.PNG, 100, out)
    return out.toByteArray()
}

/** Decodes [uri], downscales until max side ≤ 1024 px and PNG ≤ 1 MB. */
internal fun decodeStamp(
    resolver: android.content.ContentResolver,
    uri: android.net.Uri,
): android.graphics.Bitmap? {
    var bmp =
        resolver.openInputStream(uri)?.use { BitmapFactory.decodeStream(it) } ?: return null
    while (maxOf(bmp.width, bmp.height) > 1024) {
        bmp =
            android.graphics.Bitmap.createScaledBitmap(
                bmp,
                (bmp.width / 2).coerceAtLeast(1),
                (bmp.height / 2).coerceAtLeast(1),
                true,
            )
    }
    while (bmp.toPng().size > 1_000_000 && bmp.width > 16) {
        bmp =
            android.graphics.Bitmap.createScaledBitmap(
                bmp,
                (bmp.width / 2).coerceAtLeast(1),
                (bmp.height / 2).coerceAtLeast(1),
                true,
            )
    }
    return bmp
}
