package io.github.nibpdf.nib

import android.graphics.RectF
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import java.io.Closeable

/** Page rotation, multiples of 90° clockwise (PDF `/Rotate`). */
public enum class PageRotation {
    R0,
    R90,
    R180,
    R270,
}

/** Per-page display metadata in PDF points (post-`/Rotate`). */
public data class PageInfo(
    val widthPt: Float,
    val heightPt: Float,
    val rotation: PageRotation,
    /** `/CropBox` ∩ `/MediaBox` in user-space points. */
    val cropBox: RectF,
)

/**
 * An open PDF document. All PDFium work is serialized on the single
 * Rust `RenderWorker` thread behind the UniFFI surface.
 *
 * The generated `uniffi.nib_ffi` types stay internal to this module.
 */
public class NibDocument private constructor(
    private val inner: uniffi.nib_ffi.NibDocument,
    /** Display metadata for every page, loaded at open. */
    public val pages: List<PageInfo>,
) : Closeable {
    /** Number of pages. */
    public val pageCount: Int get() = pages.size

    /**
     * Bumps the scheduling generation; queued stale-generation tiles are
     * skipped. Returns the new process-wide generation for tagging requests.
     */
    public fun bumpGeneration(): ULong = inner.bumpGeneration()

    /**
     * Text inside a user-space rect on [page] (concatenated, content order).
     * Coordinates are PDF points, y-up.
     */
    public fun textInRect(
        page: Int,
        rect: RectF,
    ): String =
        inner.textInRect(
            page.toUInt(),
            uniffi.nib_ffi.Rect(
                rect.left.toDouble(),
                rect.top.toDouble(),
                rect.right.toDouble(),
                rect.bottom.toDouble(),
            ),
        )

    /** Releases the document handle. Rendering after close fails. */
    override fun close() {
        runCatching { inner.closeDoc() }
        inner.destroy()
    }

    internal suspend fun renderTile(req: uniffi.nib_ffi.TileRequest): uniffi.nib_ffi.TileResult = inner.renderTile(req)

    public companion object {
        /** Opens a document from bytes; page metadata is loaded eagerly. */
        public suspend fun open(bytes: ByteArray): NibDocument =
            withContext(Dispatchers.Default) {
                Nib // ensures libpdfium + libnib_ffi are loaded
                val inner =
                    uniffi.nib_ffi.NibDocument.fromBytes(
                        bytes,
                        uniffi.nib_ffi.OpenOptions(lazyStructure = false),
                    )
                try {
                    val count = inner.pageCount().toInt()
                    val pages =
                        (0 until count).map { i ->
                            val p = inner.pageInfo(i.toUInt())
                            PageInfo(
                                widthPt = p.widthPt.toFloat(),
                                heightPt = p.heightPt.toFloat(),
                                rotation =
                                    when (p.rotation) {
                                        uniffi.nib_ffi.Rotation.R0 -> PageRotation.R0
                                        uniffi.nib_ffi.Rotation.R90 -> PageRotation.R90
                                        uniffi.nib_ffi.Rotation.R180 -> PageRotation.R180
                                        uniffi.nib_ffi.Rotation.R270 -> PageRotation.R270
                                    },
                                cropBox =
                                    RectF(
                                        p.cropBox.x0.toFloat(),
                                        p.cropBox.y0.toFloat(),
                                        p.cropBox.x1.toFloat(),
                                        p.cropBox.y1.toFloat(),
                                    ),
                            )
                        }
                    NibDocument(inner, pages)
                } catch (t: Throwable) {
                    runCatching { inner.destroy() }
                    throw t
                }
            }
    }
}
