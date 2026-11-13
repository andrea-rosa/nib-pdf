package io.github.nibpdf.nib.ui

import androidx.compose.ui.graphics.Color

/** Parses a `#RRGGBB` schema colour into a Compose [Color], applying [opacity]. */
internal fun String.toComposeColor(opacity: Float = 1f): Color {
    val rgb = removePrefix("#").toLongOrNull(16) ?: 0L
    return Color(
        red = ((rgb shr 16) and 0xFF) / 255f,
        green = ((rgb shr 8) and 0xFF) / 255f,
        blue = (rgb and 0xFF) / 255f,
        alpha = opacity.coerceIn(0f, 1f),
    )
}
