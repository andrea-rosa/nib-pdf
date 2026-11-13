package io.github.nibpdf.nib.ui.overlay

import androidx.compose.foundation.Canvas
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.size
import androidx.compose.runtime.Composable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.rememberTextMeasurer
import androidx.compose.ui.unit.IntOffset
import io.github.nibpdf.nib.delta.Annotation
import io.github.nibpdf.nib.render.TileKey
import io.github.nibpdf.nib.render.ZoomBuckets
import io.github.nibpdf.nib.ui.NibPdfViewState
import io.github.nibpdf.nib.ui.PageTransform
import io.github.nibpdf.nib.ui.toComposeRect
import kotlin.math.ceil
import kotlin.math.floor
import kotlin.math.roundToInt

/**
 * Draws committed annotations on [page] in z-order, plus one
 * non-interactive semantics box per drawn annotation for tests.
 *
 * Overlay-until-flushed gate: an annotation is drawn while it is
 * uncommitted, or while not every intersecting tile of the current bucket
 * has been re-rendered at or after [state]'s document revision. Once the
 * flushed appearance is visible in all covering tiles the overlay drops it.
 */
@Composable
@Suppress("ktlint:standard:function-naming")
internal fun AnnotationOverlay(
    state: NibPdfViewState,
    page: Int,
    tileRevisions: Map<TileKey, Int>,
    modifier: Modifier = Modifier,
) {
    val version by state.document.annotationsVersion.collectAsState()
    val uncommitted by state.document.uncommittedIds.collectAsState()
    val revision by state.document.revision.collectAsState()
    val annotations = remember(version, page) { state.document.annotations(page) }
    val t = state.transform(page)
    val k = t.k
    val bucket = state.bucket
    val density = LocalDensity.current.density
    // Device-px transform matching the bucket's tile grid (scale*density
    // px per pt), used to find which tiles intersect each annotation.
    val scale = ZoomBuckets.scaleFor(bucket)
    val info = state.document.pages[page]
    val devT = PageTransform(info.cropBox.toComposeRect(), info.rotation, scale * density)
    val pageDevW = ceil(devT.contentSize.width.toDouble()).toInt()
    val pageDevH = ceil(devT.contentSize.height.toDouble()).toInt()
    val textMeasurer = rememberTextMeasurer()
    val images = remember { StampImageCache() }

    fun covered(a: Annotation): Boolean {
        val r = a.rect ?: return false
        val dev = devT.rectToContent(r.toComposeRect())
        val tx0 = (floor(dev.left / ZoomBuckets.TILE_SIZE).toInt()).coerceIn(0, maxOf(0, (pageDevW - 1) / ZoomBuckets.TILE_SIZE))
        val ty0 = (floor(dev.top / ZoomBuckets.TILE_SIZE).toInt()).coerceIn(0, maxOf(0, (pageDevH - 1) / ZoomBuckets.TILE_SIZE))
        val tx1 = (floor((dev.right - 1f) / ZoomBuckets.TILE_SIZE).toInt()).coerceIn(tx0, maxOf(tx0, (pageDevW - 1) / ZoomBuckets.TILE_SIZE))
        val ty1 = (floor((dev.bottom - 1f) / ZoomBuckets.TILE_SIZE).toInt()).coerceIn(ty0, maxOf(ty0, (pageDevH - 1) / ZoomBuckets.TILE_SIZE))
        for (ty in ty0..ty1) {
            for (tx in tx0..tx1) {
                // Missing tiles count as not covered: keep drawing.
                if ((tileRevisions[TileKey(page, bucket, tx, ty)] ?: 0) < revision) {
                    return false
                }
            }
        }
        return true
    }

    val drawn =
        annotations.filter { a ->
            a.id in uncommitted || !covered(a)
        }

    Canvas(modifier.fillMaxSize()) {
        drawn.forEach { a ->
            AnnotationDrawing.drawAnnotation(this, a, t, k, textMeasurer, images)
        }
    }
    drawn.forEach { a ->
        val r = a.rect?.let { t.rectToContent(it.toComposeRect()) }
        with(LocalDensity.current) {
            Box(
                Modifier
                    .offset {
                        if (r != null) {
                            IntOffset(r.left.roundToInt(), r.top.roundToInt())
                        } else {
                            IntOffset.Zero
                        }
                    }.size(
                        (r?.width ?: 1f).toDp(),
                        (r?.height ?: 1f).toDp(),
                    ).testTag("annot:${a.id}")
                    .semantics { contentDescription = "annotation:${a.subtype}" },
            )
        }
    }
}
