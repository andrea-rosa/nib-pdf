package io.github.nibpdf.nib

import android.graphics.PointF
import android.graphics.RectF
import io.github.nibpdf.nib.delta.Annotation
import io.github.nibpdf.nib.delta.ApplyReport
import io.github.nibpdf.nib.delta.DeltaDocument
import io.github.nibpdf.nib.delta.DeltaOp
import io.github.nibpdf.nib.delta.DeltaValidationError
import io.github.nibpdf.nib.delta.DirtyEntry
import io.github.nibpdf.nib.delta.DirtyRegion
import io.github.nibpdf.nib.delta.FlushMode
import io.github.nibpdf.nib.delta.FlushReport
import io.github.nibpdf.nib.delta.FormField
import io.github.nibpdf.nib.delta.InvalidDeltaException
import io.github.nibpdf.nib.delta.NibJson
import io.github.nibpdf.nib.delta.RevisionMismatchException
import io.github.nibpdf.nib.delta.SignedDocumentException
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.withContext
import kotlinx.serialization.builtins.ListSerializer
import kotlinx.serialization.decodeFromString
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.contentOrNull
import kotlinx.serialization.json.floatOrNull
import kotlinx.serialization.json.intOrNull
import kotlinx.serialization.json.jsonArray
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import java.io.Closeable
import java.io.IOException
import java.io.OutputStream

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
    private val revisionFlow = MutableStateFlow(inner.revision().toInt())
    private val uncommittedIdsFlow = MutableStateFlow(inner.uncommittedIds().toSet())
    private val dirtyLogInternal = mutableListOf<DirtyEntry>()

    /** Writer revision each annotation was last committed at (0 = imported). */
    private val commitRevision = HashMap<String, Int>()

    /** Whether [undo] currently has a step to pop. */
    public val undoAvailable: StateFlow<Boolean> get() = undoAvailableFlow

    /** Whether [redo] currently has a step to pop. */
    public val redoAvailable: StateFlow<Boolean> get() = redoAvailableFlow

    /** Bumped after every mutation; observers reload model state (annotations + form fields). */
    public val annotationsVersion: StateFlow<Int> get() = annotationsVersionFlow

    /** Writer revision (0 before the first [flush]). */
    public val revision: StateFlow<Int> get() = revisionFlow

    /** Ids of annotations whose changes haven't been flushed yet. */
    public val uncommittedIds: StateFlow<Set<String>> get() = uncommittedIdsFlow

    /** Number of uncommitted annotations. */
    public val uncommittedCount: Int get() = uncommittedIdsFlow.value.size

    /**
     * Delta ops applied since the last [flush]; resets to 0 on flush. Used
     * by `FlushPolicy.maxOps`.
     */
    public var opsSinceFlush: Int = 0
        internal set

    /** Page rects each flush published, in flush order. */
    public val dirtyLog: List<DirtyEntry> get() = dirtyLogInternal

    /**
     * Writer revision [id] was last written into the PDF bytes; 0 for
     * annotations imported at open (or never flushed). Used by the overlay
     * gate so untouched imported annotations aren't double-drawn.
     */
    public fun commitRevisionOf(id: String): Int = commitRevision[id] ?: 0

    private fun refresh() {
        undoAvailableFlow.value = inner.canUndo()
        redoAvailableFlow.value = inner.canRedo()
        revisionFlow.value = inner.revision().toInt()
        uncommittedIdsFlow.value = inner.uncommittedIds().toSet()
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
        opsSinceFlush += ops.size
        refresh()
        return report.toPublic()
    }

    /** Applies a single op. */
    public fun apply(op: DeltaOp): ApplyReport = apply(listOf(op))

    /** Pops one undo step; returns the inverse ops applied, or `null`. */
    public fun undo(): List<DeltaOp>? {
        val json = inner.undo() ?: return null
        val ops: List<DeltaOp> = NibJson.decodeFromString(json)
        opsSinceFlush += ops.size
        refresh()
        return ops
    }

    /** Pops one redo step; returns the forward ops re-applied, or `null`. */
    public fun redo(): List<DeltaOp>? {
        val json = inner.redo() ?: return null
        val ops: List<DeltaOp> = NibJson.decodeFromString(json)
        opsSinceFlush += ops.size
        refresh()
        return ops
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

    // ---- AcroForm fields ------------------------------------------------

    /** All form fields, in document order (empty after a flattened flush). */
    public fun formFields(): List<FormField> = inner.formFields().map(FormField::fromSummary)

    /** One field by fully-qualified name, or `null`. */
    public fun formField(name: String): FormField? =
        inner.formFieldJson(name)?.let {
            FormField.fromJson(NibJson.parseToJsonElement(it).jsonObject)
        }

    /**
     * Sets field [name]'s `value` (a `JsonPrimitive` string, `JsonArray` of
     * strings for multi-select lists, or `JsonNull` to clear).
     *
     * @throws InvalidDeltaException on validation failures
     *   (`FIELD_READONLY`, `VALUE_NOT_IN_OPTIONS`, `MAXLEN_EXCEEDED`, …)
     */
    public fun setFieldValue(
        name: String,
        value: JsonElement,
    ): ApplyReport {
        val report =
            try {
                inner.setFieldValue(name, value.toString())
            } catch (e: uniffi.nib_ffi.NibException.InvalidDelta) {
                throw InvalidDeltaException(
                    NibJson.decodeFromString<List<DeltaValidationError>>(e.v1),
                )
            } catch (e: uniffi.nib_ffi.NibException.RevisionMismatch) {
                throw RevisionMismatchException(e.expected.toInt(), e.got.toInt())
            }
        opsSinceFlush++
        refresh()
        return report.toPublic()
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
        opsSinceFlush +=
            runCatching { NibJson.decodeFromString<DeltaDocument>(json).ops.size }
                .getOrDefault(report.applied.size)
        refresh()
        return report.toPublic()
    }

    // ---- Persistence ---------------------------------------------------

    /** Supplies foreign assets (stamp images) during appearance generation. */
    public fun interface AssetProvider {
        /**
         * Returns the bytes of asset [assetId], or `null`. Runs
         * synchronously on the calling thread while the engine lock is
         * held — must not call back into [NibDocument].
         */
        public fun asset(assetId: String): ByteArray?
    }

    /** Supplies fonts during appearance generation. */
    public fun interface FontProvider {
        /**
         * Returns the font bytes for ([family], [bold], [italic]), or
         * `null`. Runs synchronously on the calling thread while the
         * engine lock is held — must not call back into [NibDocument].
         */
        public fun font(
            family: String,
            bold: Boolean,
            italic: Boolean,
        ): ByteArray?
    }

    /** Registers the stamp-image provider used by the next [flush]. */
    public fun setAssetProvider(p: AssetProvider?) {
        inner.setAssetProvider(
            p?.let { provider ->
                object : uniffi.nib_ffi.AssetProvider {
                    override fun asset(assetId: String): ByteArray? = provider.asset(assetId)
                }
            },
        )
    }

    /** Registers the font provider used by the next [flush]. */
    public fun setFontProvider(p: FontProvider?) {
        inner.setFontProvider(
            p?.let { provider ->
                object : uniffi.nib_ffi.FontProvider {
                    override fun font(
                        family: String,
                        bold: Boolean,
                        italic: Boolean,
                    ): ByteArray? = provider.font(family, bold, italic)
                }
            },
        )
    }

    private fun uniffi.nib_ffi.FlushReport.toPublic(): FlushReport =
        FlushReport(
            revision = revision.toInt(),
            bytesWritten = bytesWritten.toLong(),
            mode =
                when (mode) {
                    uniffi.nib_ffi.FlushMode.INCREMENTAL -> FlushMode.Incremental
                    uniffi.nib_ffi.FlushMode.FULL_REWRITE -> FlushMode.FullRewrite
                },
            annotsWritten = annotsWritten.toInt(),
            fieldsWritten = fieldsWritten.toInt(),
            flattened = flattened,
            warnings = warnings,
            dirty = dirty.map { DirtyRegion(it.page.toInt(), it.rect.toRectF()) },
        )

    /**
     * Writes the uncommitted annotations and field values into the PDF
     * bytes. [flatten] burns widget appearances into page contents and
     * drops the AcroForm (model fields become empty).
     *
     * @throws SignedDocumentException when [mode] is
     *   [FlushMode.FullRewrite] and the document is signed
     * @throws IOException on other write failures
     */
    public fun flush(
        mode: FlushMode = FlushMode.Incremental,
        flatten: Boolean = false,
    ): FlushReport {
        val written = uncommittedIdsFlow.value
        val report =
            try {
                inner.flushWith(
                    uniffi.nib_ffi.FlushOptions(
                        mode =
                            when (mode) {
                                FlushMode.Incremental -> uniffi.nib_ffi.FlushMode.INCREMENTAL
                                FlushMode.FullRewrite -> uniffi.nib_ffi.FlushMode.FULL_REWRITE
                            },
                        flatten = flatten,
                    ),
                )
            } catch (e: uniffi.nib_ffi.NibException.SignedDocument) {
                throw SignedDocumentException()
            } catch (e: uniffi.nib_ffi.NibException.WriteFailed) {
                throw IOException(e.v1)
            } catch (e: uniffi.nib_ffi.NibException.Io) {
                throw IOException(e.v1)
            }
        val pub = report.toPublic()
        pub.dirty.forEach { dirtyLogInternal += DirtyEntry(pub.revision, it.page, it.rect) }
        for (id in written) {
            commitRevision[id] = pub.revision
        }
        // Drop entries for entities no longer in the model; after flatten
        // every form field name is gone, so their commits are pruned too.
        commitRevision.keys.retainAll {
            inner.annotationJson(it) != null || inner.formFieldJson(it) != null
        }
        opsSinceFlush = 0
        refresh()
        return pub
    }

    /**
     * Flushes, then streams the current PDF bytes to [target] (closed
     * afterwards). [optimized] selects a compacting full rewrite; throws
     * [SignedDocumentException] if the document is signed.
     */
    public suspend fun save(
        target: OutputStream,
        optimized: Boolean = false,
        flatten: Boolean = false,
    ): FlushReport =
        withContext(Dispatchers.IO) {
            val r =
                flush(
                    if (optimized) FlushMode.FullRewrite else FlushMode.Incremental,
                    flatten = flatten,
                )
            target.use { it.write(bytes()) }
            r
        }

    /**
     * Current PDF bytes (post-flush revisions included).
     *
     * @throws IOException when the underlying writer failed at open
     */
    public fun bytes(): ByteArray =
        try {
            inner.bytes()
        } catch (e: uniffi.nib_ffi.NibException.WriteFailed) {
            throw IOException(e.v1)
        } catch (e: uniffi.nib_ffi.NibException.Io) {
            throw IOException(e.v1)
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
