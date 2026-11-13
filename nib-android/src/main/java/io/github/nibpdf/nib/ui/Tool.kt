package io.github.nibpdf.nib.ui

/** Active editing tool (JSON_SCHEMA §5 subtypes). */
public enum class Tool {
    Pan,
    Select,
    Ink,
    Highlight,
    Underline,
    StrikeOut,
    Square,
    Circle,
    Line,
    FreeText,
    Note,
    Stamp,
}

/** Style applied to newly created annotations. Colours are `#RRGGBB`. */
public data class ToolStyle(
    val color: String = "#1A237E",
    val strokeWidth: Float = 2f,
    val opacity: Float = 1f,
    val fontSize: Float = 12f,
    val fillColor: String? = null,
)

/** What a `Stamp` gesture should drop. */
public sealed class StampSpec {
    public data class Image(
        val png: ByteArray,
        val widthPx: Int,
        val heightPx: Int,
        val mime: String = "image/png",
    ) : StampSpec()

    public data class Standard(
        val name: String,
    ) : StampSpec()
}
