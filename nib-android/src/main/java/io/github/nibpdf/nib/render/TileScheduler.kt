package io.github.nibpdf.nib.render

import android.graphics.Bitmap
import io.github.nibpdf.nib.NibDocument
import io.github.nibpdf.nib.ui.PageLayout
import kotlin.math.ceil
import kotlin.math.roundToInt
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Job
import kotlinx.coroutines.flow.MutableSharedFlow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.SharedFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.launch
import uniffi.nib_ffi.NibException

/**
 * Computes wanted tiles from the viewport, launches render jobs
 * (visible first, then a one-tile prefetch ring plus the first row of the
 * next page), cancels jobs that fall out of the wanted set, and bumps the
 * render generation on bucket changes.
 */
internal class TileScheduler(
    private val document: NibDocument,
    private val cache: TileCache<TileKey, Bitmap>,
    private val pool: BitmapPool,
    private val scope: CoroutineScope,
    private val density: Float,
) {
    private val jobs = HashMap<TileKey, Job>()
    private var generation: ULong = 0u
    private var lastBucket: Int? = null

    /** Emitted for every tile job launched (used by ZoomTest). */
    private val _tileRequests = MutableSharedFlow<TileKey>(replay = 64)
    val tileRequests: SharedFlow<TileKey> = _tileRequests

    /** Bumped on each completed tile so the view recomposes. */
    private val _tilesChanged = MutableStateFlow(0)
    val tilesChanged: StateFlow<Int> = _tilesChanged

    /** Snapshot of cached bitmaps for one page at one bucket. */
    fun tilesForPage(
        page: Int,
        bucket: Int,
    ): Map<TileKey, Bitmap> = cacheSnapshot().filterKeys { it.page == page && it.bucket == bucket }

    fun get(key: TileKey): Bitmap? = cache[key]

    private fun cacheSnapshot(): Map<TileKey, Bitmap> = cache.snapshot()

    /**
     * Recomputes the wanted set for `[viewportTop, viewportBottom)` content px
     * and reconciles running jobs.
     */
    fun update(
        layout: PageLayout,
        viewportTop: Float,
        viewportBottom: Float,
        viewportWidth: Float,
        zoom: Float,
        bucket: Int,
    ) {
        if (lastBucket != bucket) {
            generation++
            document.bumpGeneration()
            // Tiles of other buckets are only useful while zooming; drop them
            // once a new bucket settles.
            cache.evictMatching { it.bucket == bucket }
            lastBucket = bucket
        }

        data class Wanted(
            val key: TileKey,
            val page: Int,
            val visible: Boolean,
            val deviceX: Int,
            val deviceY: Int,
            val deviceW: Int,
            val deviceH: Int,
            val pageDevW: Int,
            val pageDevH: Int,
            val distance: UInt,
        )

        val scale = ZoomBuckets.scaleFor(bucket)
        val wanted = LinkedHashMap<TileKey, Wanted>()
        val pages = layout.pagesIn(viewportTop, viewportBottom, zoom)
        if (pages.isEmpty()) return
        val viewportCentreY = (viewportTop + viewportBottom) / 2f
        val viewportCentreX = viewportWidth / 2f

        fun pageTiles(
            page: Int,
            yFromContent: Float,
            yToContent: Float,
            visible: Boolean,
        ) {
            val devW = ceil(document.pages[page].widthPt * scale * density).toInt()
            val devH = ceil(document.pages[page].heightPt * scale * density).toInt()
            if (devW <= 0 || devH <= 0) return
            val tilesX = ceil(devW.toDouble() / ZoomBuckets.TILE_SIZE).toInt()
            val nTilesY = ceil(devH.toDouble() / ZoomBuckets.TILE_SIZE).toInt()
            val pageTopContent = layout.pageTop(page, zoom)
            val devPerContent = scale / zoom // device px per content px
            val ty0 =
                ((yFromContent - pageTopContent) * devPerContent / ZoomBuckets.TILE_SIZE)
                    .toInt()
                    .coerceIn(0, nTilesY - 1)
            val ty1 =
                ceil((yToContent - pageTopContent) * devPerContent / ZoomBuckets.TILE_SIZE)
                    .toInt()
                    .coerceIn(0, nTilesY)
            for (ty in ty0 until ty1) {
                for (tx in 0 until tilesX) {
                    val cxDev = (tx + 0.5f) * ZoomBuckets.TILE_SIZE
                    val cyDev = (ty + 0.5f) * ZoomBuckets.TILE_SIZE
                    val dx = cxDev - viewportCentreX * devPerContent
                    val dy = cyDev - (viewportCentreY - pageTopContent) * devPerContent
                    val dist = (dx * dx + dy * dy).roundToInt().coerceAtLeast(0)
                    val key = TileKey(page, bucket, tx, ty)
                    wanted[key] =
                        Wanted(
                            key,
                            page,
                            visible,
                            tx * ZoomBuckets.TILE_SIZE,
                            ty * ZoomBuckets.TILE_SIZE,
                            minOf(ZoomBuckets.TILE_SIZE, devW - tx * ZoomBuckets.TILE_SIZE),
                            minOf(ZoomBuckets.TILE_SIZE, devH - ty * ZoomBuckets.TILE_SIZE),
                            devW,
                            devH,
                            dist.toUInt(),
                        )
                }
            }
        }

        // Visible tiles.
        for (page in pages) {
            val top = maxOf(viewportTop, layout.pageTop(page, zoom))
            val bottom = minOf(viewportBottom, layout.pageTop(page, zoom) + layout.pageHeightPx(page, zoom))
            pageTiles(page, top, bottom, visible = true)
        }
        // Prefetch ring: one tile row/col beyond visible on the same pages.
        for (page in pages) {
            val pad = ZoomBuckets.TILE_SIZE / (scale / zoom)
            val top = maxOf(viewportTop - pad, layout.pageTop(page, zoom))
            val bottom =
                minOf(
                    viewportBottom + pad,
                    layout.pageTop(page, zoom) + layout.pageHeightPx(page, zoom),
                )
            pageTiles(page, top, bottom, visible = false)
        }
        // Prefetch: first row of the next page in scroll direction.
        val nextPage = pages.last + 1
        if (nextPage < document.pageCount) {
            val top = layout.pageTop(nextPage, zoom)
            pageTiles(nextPage, top, top + ZoomBuckets.TILE_SIZE / (scale / zoom), visible = false)
        }

        // Cancel jobs no longer wanted.
        val stale = jobs.keys.filterNot { wanted.containsKey(it) }
        stale.forEach { jobs.remove(it)?.cancel() }

        // Launch missing jobs: visible first.
        val toLaunch =
            wanted.values
                .filter { !jobs.containsKey(it.key) && cache[it.key] == null }
                .sortedByDescending { it.visible }
        for (w in toLaunch) {
            val req =
                uniffi.nib_ffi.TileRequest(
                    key =
                        uniffi.nib_ffi.TileKey(
                            page = w.key.page.toUShort(),
                            bucket = w.key.bucket.toByte(),
                            tx = w.key.tx.toUShort(),
                            ty = w.key.ty.toUShort(),
                            revision = 0u,
                            flags = 0u,
                        ),
                    deviceRect =
                        uniffi.nib_ffi.IRect(
                            x = w.deviceX,
                            y = w.deviceY,
                            width = w.deviceW,
                            height = w.deviceH,
                        ),
                    pageDeviceSize =
                        uniffi.nib_ffi.ISize(
                            width = w.pageDevW,
                            height = w.pageDevH,
                        ),
                    generation = generation,
                    priority =
                        if (w.visible) {
                            uniffi.nib_ffi.Priority.VISIBLE
                        } else {
                            uniffi.nib_ffi.Priority.PREFETCH
                        },
                    distance = w.distance,
                )
            val key = w.key
            _tileRequests.tryEmit(key)
            jobs[key] =
                scope.launch {
                    try {
                        val bmp = document.renderTile(req).toBitmap(pool)
                        cache.put(key, bmp)
                    } catch (e: NibException) {
                        // Cancelled / stale generation: nothing to store.
                    } finally {
                        jobs.remove(key)
                        _tilesChanged.value++
                    }
                }
        }
    }

    fun cancelAll() {
        jobs.values.forEach { it.cancel() }
        jobs.clear()
    }
}
