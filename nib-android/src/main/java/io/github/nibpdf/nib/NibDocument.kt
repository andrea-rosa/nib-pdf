package io.github.nibpdf.nib

import android.graphics.PointF
import android.graphics.RectF
import io.github.nibpdf.nib.delta.Annotation
import io.github.nibpdf.nib.delta.ApplyReport
import io.github.nibpdf.nib.delta.DeltaOp
import io.github.nibpdf.nib.delta.DeltaValidationError
import io.github.nibpdf.nib.delta.DirtyRegion
import io.github.nibpdf.nib.delta.InvalidDeltaException
import io.github.nibpdf.nib.delta.NibJson
import io.github.nibpdf.nib.delta.RevisionMismatchException
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.withContext
import kotlinx.serialization.builtins.ListSerializer
import kotlinx.serialization.decodeFromString
import kotlinx.serialization.json.contentOrNull
import kotlinx.serialization.json.floatOrNull
import kotlinx.serialization.json.intOrNull
import kotlinx.serialization.json.jsonArray
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
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

    // ---- Delta facade -------------------------------------------------

    /** Export granularity for [exportDelta]. */
    public enum class ExportMode {
        Full,
        Compacted,
    }

    /** Import behaviour switches (JSON_SCHEMA §8). */
    public data class ImportOptions(
        val force: Boolean = false,
        val rebase: Boolean = false,
        val undoable: Boolean = true,
    )

    private val undoAvailableFlow = MutableStateFlow(false)
    private val redoAvailableFlow = MutableStateFlow(false)
    private val annotationsVersionFlow = MutableStateFlow(0)

    /** Whether [undo] currently has a step to pop. */
    public val undoAvailable: StateFlow<Boolean> get() = undoAvailableFlow

    /** Whether [redo] currently has a step to pop. */
    public val redoAvailable: StateFlow<Boolean> get() = redoAvailableFlow

    /** Bumped after every mutation; observers reload annotation state. */
    public val annotationsVersion: StateFlow<Int> get() = annotationsVersionFlow

    private fun refresh() {
        undoAvailableFlow.value = inner.canUndo()
        redoAvailableFlow.value = inner.canRedo()
        annotationsVersionFlow.value += 1
    }

    private fun uniffi.nib_ffi.Rect.toRectF(): RectF = RectF(x0.toFloat(), y0.toFloat(), x1.toFloat(), y1.toFloat())

    private fun uniffi.nib_ffi.ApplyReport.toPublic(): ApplyReport =
        ApplyReport(
            applied = applied,
            skipped = skipped,
            rejected =
                rejected.map {
                    DeltaValidationError(it.opIndex.toInt(), it.path, it.code, it.message)
                },
            dirty = dirty.map { DirtyRegion(it.page.toInt(), it.rect.toRectF()) },
        )

    /** Applies [ops] as one undo step. @throws InvalidDeltaException, RevisionMismatchException */
    public fun apply(ops: List<DeltaOp>): ApplyReport {
        val json = NibJson.encodeToString(ListSerializer(DeltaOp.serializer()), ops)
        val report =
            try {
                inner.apply(json)
            } catch (e: uniffi.nib_ffi.NibException.InvalidDelta) {
                throw InvalidDeltaException(
                    NibJson.decodeFromString<List<DeltaValidationError>>(e.v1),
                )
            } catch (e: uniffi.nib_ffi.NibException.RevisionMismatch) {
                throw RevisionMismatchException(e.expected.toInt(), e.got.toInt())
            }
        refresh()
        return report.toPublic()
    }

    /** Applies a single op. */
    public fun apply(op: DeltaOp): ApplyReport = apply(listOf(op))

    /** Pops one undo step; returns the inverse ops applied, or `null`. */
    public fun undo(): List<DeltaOp>? {
        val json = inner.undo() ?: return null
        refresh()
        return NibJson.decodeFromString(json)
    }

    /** Pops one redo step; returns the forward ops re-applied, or `null`. */
    public fun redo(): List<DeltaOp>? {
        val json = inner.redo() ?: return null
        refresh()
        return NibJson.decodeFromString(json)
    }

    /** Opens a coalescing group; returns its `groupId`. */
    public fun beginGroup(): String = inner.beginGroup()

    /** Closes the open group (pushes it as one undo step). */
    public fun endGroup() {
        inner.endGroup()
        refresh()
    }

    /** All annotations on [page], in z-order. */
    public fun annotations(page: Int): List<Annotation> = inner.annotations(page.toUInt()).map(Annotation::fromSummary)

    /** One annotation by id, or `null`. */
    public fun annotation(id: String): Annotation? =
        inner.annotationJson(id)?.let { json ->
            val obj = NibJson.parseToJsonElement(json).jsonObject
            val r = obj["rect"]?.jsonArray
            Annotation(
                id = obj["id"]?.jsonPrimitive?.contentOrNull ?: id,
                subtype = obj["subtype"]?.jsonPrimitive?.contentOrNull ?: "",
                page = obj["page"]?.jsonPrimitive?.intOrNull ?: 0,
                rect =
                    r?.let { a ->
                        RectF(
                            a[0].jsonPrimitive.floatOrNull ?: 0f,
                            a[1].jsonPrimitive.floatOrNull ?: 0f,
                            a[2].jsonPrimitive.floatOrNull ?: 0f,
                            a[3].jsonPrimitive.floatOrNull ?: 0f,
                        )
                    },
                json = obj,
            )
        }

    /** Hit-tests [pt] on [page]; returns the topmost annotation id or `null`. */
    public fun hitTest(
        page: Int,
        pt: PointF,
        tolerancePt: Float,
    ): String? =
        inner.hitTest(
            page.toUInt(),
            uniffi.nib_ffi.Point(pt.x.toDouble(), pt.y.toDouble()),
            tolerancePt.toDouble(),
        )

    /** Serialises the delta history (JSON_SCHEMA §8). */
    public fun exportDelta(mode: ExportMode): String =
        inner.exportDelta(
            when (mode) {
                ExportMode.Full -> uniffi.nib_ffi.ExportMode.FULL
                ExportMode.Compacted -> uniffi.nib_ffi.ExportMode.COMPACTED
            },
        )

    /** Imports a delta document. @throws InvalidDeltaException, RevisionMismatchException */
    public fun importDelta(
        json: String,
        options: ImportOptions = ImportOptions(),
    ): ApplyReport {
        val report =
            try {
                inner.importDelta(
                    json,
                    uniffi.nib_ffi.ImportOptions(options.force, options.rebase, options.undoable),
                )
            } catch (e: uniffi.nib_ffi.NibException.InvalidDelta) {
                throw InvalidDeltaException(
                    NibJson.decodeFromString<List<DeltaValidationError>>(e.v1),
                )
            } catch (e: uniffi.nib_ffi.NibException.RevisionMismatch) {
                throw RevisionMismatchException(e.expected.toInt(), e.got.toInt())
            }
        refresh()
        return report.toPublic()
    }

    /**
     * Text line rects snapped to glyph boxes inside [rect] on [page]
     * (user-space points, y-up, `left/top/right/bottom` = `x0,y0,x1,y1`).
     */
    public suspend fun textLinesInRect(
        page: Int,
        rect: RectF,
    ): List<RectF> =
        withContext(Dispatchers.Default) {
            inner
                .textLinesInRect(
                    page.toUInt(),
                    uniffi.nib_ffi.Rect(
                        rect.left.toDouble(),
                        rect.top.toDouble(),
                        rect.right.toDouble(),
                        rect.bottom.toDouble(),
                    ),
                ).map { it.toRectF() }
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
