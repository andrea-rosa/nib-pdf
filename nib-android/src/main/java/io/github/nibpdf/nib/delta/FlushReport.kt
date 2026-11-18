package io.github.nibpdf.nib.delta

import android.graphics.RectF
import java.io.IOException

/** How a flush writes the PDF (incremental append vs. compacting rewrite). */
public enum class FlushMode {
    Incremental,
    FullRewrite,
}

/** Result of one `NibDocument.flush`. */
public data class FlushReport(
    val revision: Int,
    val bytesWritten: Long,
    val mode: FlushMode,
    val annotsWritten: Int,
    /** AcroForm fields written by this flush. */
    val fieldsWritten: Int = 0,
    /** True when widgets were flattened into page contents. */
    val flattened: Boolean = false,
    val warnings: List<String>,
    val dirty: List<DirtyRegion>,
)

/**
 * One page rect dirtied by a flush, tagged with the writer revision that
 * published it. Tiles rendered at a revision `<` the entry's are stale for
 * that rect and must be re-rendered.
 */
public data class DirtyEntry(
    val revision: Int,
    val page: Int,
    /** User-space rect (PDF points, y-up). */
    val rect: RectF,
)

/** Thrown when a full rewrite is refused because the document is signed. */
public class SignedDocumentException : IOException("document has signatures; full rewrite refused")
