package io.github.nibpdf.nib.render

import android.graphics.Bitmap
import android.graphics.RectF
import androidx.annotation.VisibleForTesting
import io.github.nibpdf.nib.NibDocument
import io.github.nibpdf.nib.ui.PageLayout
import io.github.nibpdf.nib.ui.PageTransform
import io.github.nibpdf.nib.ui.toComposeRect
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Job
import kotlinx.coroutines.flow.MutableSharedFlow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.SharedFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.launch
import uniffi.nib_ffi.NibException
import kotlin.math.ceil
import kotlin.math.roundToInt

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

    /** Writer revision each in-flight job was requested at. */
    private val jobRevision = HashMap<TileKey, Int>()
    private var generation: ULong = 0u
    private var lastBucket: Int? = null

    /** Writer revision each cached tile was rendered at. */
    private val tileRevision = java.util.concurrent.ConcurrentHashMap<TileKey, Int>()

    /**
     * When true, [update] launches no new render jobs (used by tests to
     * observe the overlay-until-flushed window). The old bitmaps keep
     * drawing until replaced.
     */
    @VisibleForTesting
    internal var paused: Boolean = false

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

    /** Render revisions of the cached tiles for one page at one bucket. */
    fun tileRevisionsForPage(
        page: Int,
        bucket: Int,
    ): Map<TileKey, Int> = tileRevision.filterKeys { it.page == page && it.bucket == bucket }

    /** Writer revision [key]'s cached tile was rendered at, if known. */
    fun revisionOf(key: TileKey): Int? = tileRevision[key]

    fun get(key: TileKey): Bitmap? = cache[key]

    /**
     * Highest flush revision whose dirty rect (converted to this tile's
     * device-px space) intersects the tile. Tiles rendered below it are
     * stale for that rect and get re-rendered.
     */
    private fun requiredRevision(
        page: Int,
        tx: Int,
        ty: Int,
        deviceW: Int,
        deviceH: Int,
        scale: Float,
    ): Int {
        val log = document.dirtyLog
        if (log.isEmpty()) return 0
        val info = document.pages[page]
        val t = PageTransform(info.cropBox.toComposeRect(), info.rotation, scale * density)
        val tileRect =
            RectF(
                (tx * ZoomBuckets.TILE_SIZE).toFloat(),
                (ty * ZoomBuckets.TILE_SIZE).toFloat(),
                minOf((tx + 1) * ZoomBuckets.TILE_SIZE, deviceW).toFloat(),
                minOf((ty + 1) * ZoomBuckets.TILE_SIZE, deviceH).toFloat(),
            )
        var required = 0
        for (e in log) {
            if (e.page != page || e.revision <= required) continue
            val dev = t.rectToContent(e.rect.toComposeRect())
            if (
                dev.left < tileRect.right &&
                dev.right > tileRect.left &&
                dev.top < tileRect.bottom &&
                dev.bottom > tileRect.top
            ) {
                required = e.revision
            }
        }
        return required
    }

    /**
     * Tiles of the cached bucket nearest to [bucket] for [page], paired with
     * that bucket. Drawn scaled while tiles of [bucket] are still rendering.
     */
    fun fallbackTilesForPage(
        page: Int,
        bucket: Int,
    ): Pair<Int, Map<TileKey, Bitmap>>? {
        val snap = cacheSnapshot()
        val nearest =
            snap.keys
                .asSequence()
                .filter { it.page == page && it.bucket != bucket }
                .map { it.bucket }
                .distinct()
                .minByOrNull { kotlin.math.abs(it - bucket) }
                ?: return null
        return nearest to snap.filterKeys { it.page == page && it.bucket == nearest }
    }

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
            // The worker generation is process-wide and monotonic; adopt its
            // new value rather than a local counter so a recreated scheduler
            // doesn't request tiles at a stale generation.
            generation = document.bumpGeneration()
            // Tiles of other buckets stay cached: the byte budget bounds them
            // and PageItem draws them scaled while the new bucket renders.
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
                    // Prefetch ranges overlap the visible range; never
                    // downgrade an already-wanted visible tile.
                    if (key in wanted) continue
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

        // Cancel jobs no longer wanted. jobs is also mutated by completed
        // render coroutines on another dispatcher; iterate over a snapshot.
        val stale = jobs.keys.toList().filterNot { wanted.containsKey(it) }
        stale.forEach {
            jobs.remove(it)?.cancel()
            jobRevision.remove(it)
        }

        if (paused) return

        // Launch missing or revision-stale jobs: visible first. A stale
        // tile's old bitmap stays cached (and keeps drawing) until the new
        // render replaces it; `cache.put` releases it via the eviction
        // callback. An in-flight job requested before the latest flush
        // would land stale with nothing to re-trigger it, so supersede it.
        val toLaunch =
            wanted.values
                .filter {
                    val required = requiredRevision(it.page, it.key.tx, it.key.ty, it.pageDevW, it.pageDevH, scale)
                    if (jobs.containsKey(it.key)) {
                        (jobRevision[it.key] ?: 0) < required
                    } else {
                        cache[it.key] == null || (tileRevision[it.key] ?: 0) < required
                    }
                }.sortedByDescending { it.visible }
        val reqRevision = document.revision.value.toUInt()
        for (w in toLaunch) {
            jobs.remove(w.key)?.cancel()
            val req =
                uniffi.nib_ffi.TileRequest(
                    key =
                        uniffi.nib_ffi.TileKey(
                            page = w.key.page.toUShort(),
                            bucket = w.key.bucket.toByte(),
                            tx = w.key.tx.toUShort(),
                            ty = w.key.ty.toUShort(),
                            revision = reqRevision,
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
            jobRevision[key] = reqRevision.toInt()
            jobs[key] =
                scope.launch {
                    try {
                        val result = document.renderTile(req)
                        val bmp = result.toBitmap(pool)
                        tileRevision[key] = result.key.revision.toInt()
                        cache.put(key, bmp)
                    } catch (e: NibException) {
                        // Cancelled / stale generation: nothing to store.
                    } catch (e: IllegalStateException) {
                        // Document closed while a job was still in flight.
                    } finally {
                        // A superseded job must not unregister its successor.
                        if (jobs[key] === coroutineContext[Job]) {
                            jobs.remove(key)
                            jobRevision.remove(key)
                        }
                        _tilesChanged.value++
                    }
                }
        }
    }

    fun cancelAll() {
        jobs.values.forEach { it.cancel() }
        jobs.clear()
        jobRevision.clear()
    }
}
