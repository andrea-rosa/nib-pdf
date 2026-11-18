package io.github.nibpdf.nib.ui.overlay

import android.graphics.RectF
import io.github.nibpdf.nib.PageInfo
import io.github.nibpdf.nib.render.TileKey
import io.github.nibpdf.nib.render.ZoomBuckets
import io.github.nibpdf.nib.ui.PageTransform
import io.github.nibpdf.nib.ui.toComposeRect
import kotlin.math.ceil
import kotlin.math.floor

/**
 * Answers "is [userRect] already baked into every covering tile of the
 * current bucket?" — used by both the annotation and the form overlay to
 * drop drawn widgets once the flushed appearance is visible.
 *
 * Tiles rendered at or after the revision an entity was committed at
 * contain its baked appearance; missing/older tiles count as uncovered.
 */
internal class TileCoverage(
    private val page: Int,
    private val bucket: Int,
    density: Float,
    pageInfo: PageInfo,
    private val tileRevisions: Map<TileKey, Int>,
) {
    private val devT =
        PageTransform(
            pageInfo.cropBox.toComposeRect(),
            pageInfo.rotation,
            ZoomBuckets.scaleFor(bucket) * density,
        )
    private val pageDevW = ceil(devT.contentSize.width.toDouble()).toInt()
    private val pageDevH = ceil(devT.contentSize.height.toDouble()).toInt()

    fun covered(
        userRect: RectF,
        commitRevision: Int,
    ): Boolean {
        val dev = devT.rectToContent(userRect.toComposeRect())
        val tx0 =
            (floor(dev.left / ZoomBuckets.TILE_SIZE).toInt())
                .coerceIn(0, maxOf(0, (pageDevW - 1) / ZoomBuckets.TILE_SIZE))
        val ty0 =
            (floor(dev.top / ZoomBuckets.TILE_SIZE).toInt())
                .coerceIn(0, maxOf(0, (pageDevH - 1) / ZoomBuckets.TILE_SIZE))
        val tx1 =
            (floor((dev.right - 1f) / ZoomBuckets.TILE_SIZE).toInt())
                .coerceIn(tx0, maxOf(tx0, (pageDevW - 1) / ZoomBuckets.TILE_SIZE))
        val ty1 =
            (floor((dev.bottom - 1f) / ZoomBuckets.TILE_SIZE).toInt())
                .coerceIn(ty0, maxOf(ty0, (pageDevH - 1) / ZoomBuckets.TILE_SIZE))
        for (ty in ty0..ty1) {
            for (tx in tx0..tx1) {
                // Missing tiles count as not covered: keep drawing.
                if ((tileRevisions[TileKey(page, bucket, tx, ty)] ?: 0) < commitRevision) {
                    return false
                }
            }
        }
        return true
    }
}
