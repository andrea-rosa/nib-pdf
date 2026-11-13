package io.github.nibpdf.nib.ui

import android.graphics.Bitmap
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.platform.LocalDensity
import io.github.nibpdf.nib.NibDocument
import io.github.nibpdf.nib.render.BitmapPool
import io.github.nibpdf.nib.render.TileCache
import io.github.nibpdf.nib.render.TileKey
import io.github.nibpdf.nib.render.TileScheduler
import io.github.nibpdf.nib.render.ZoomBuckets
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.flow.SharedFlow
import kotlinx.coroutines.flow.StateFlow

/**
 * Hoisted state for [NibPdfView]: zoom, scroll, scheduler, cache.
 *
 * Content px = pt × density × zoom (1 pt ≙ 1 dp at zoom 1).
 */
public class NibPdfViewState internal constructor(
    public val document: NibDocument,
    density: Float,
    scope: CoroutineScope,
) {
    /** Continuous zoom, clamped to `ZoomBuckets.MIN_ZOOM..MAX_ZOOM`. */
    public var zoom: Float by mutableFloatStateOf(1f)

    /** Vertical scroll offset in content px. */
    public var scrollY: Float by mutableFloatStateOf(0f)

    /** Active zoom bucket (`scale = 2^(bucket/2)`). */
    public val bucket: Int get() = ZoomBuckets.bucketFor(zoom)

    /** Emitted for every tile render job the scheduler launches. */
    public val tileRequests: SharedFlow<TileKey>

    /** Viewport size in px; set by the view's `onSizeChanged`. */
    internal var viewportWidth: Int by mutableIntStateOf(0)
    internal var viewportHeight: Int by mutableIntStateOf(0)

    internal val layout = PageLayout(document.pages, density, gapPx = 8f * density)
    internal val cache =
        TileCache<TileKey, Bitmap>(TileCache.defaultBudget(), { it.byteCount }) { bmp ->
            pool.recycle(bmp)
        }
    private val pool = BitmapPool()
    internal val scheduler = TileScheduler(document, cache, pool, scope, density)
    internal val tilesChanged: StateFlow<Int> get() = scheduler.tilesChanged

    init {
        tileRequests = scheduler.tileRequests
    }

    /** Largest legal `scrollY` (end of content). */
    public fun maxScrollY(): Float = (layout.totalHeight(zoom) - viewportHeight).coerceAtLeast(0f)

    /** Scrolls to `y` content px, clamped. */
    public fun scrollTo(y: Float) {
        scrollY = y.coerceIn(0f, maxScrollY())
    }

    /** Applies `ComponentCallbacks2` trim levels to the tile cache. */
    public fun onTrimMemory(level: Int) {
        cache.trim(level)
    }

    internal fun updateScheduler() {
        if (viewportHeight <= 0) return
        scheduler.update(
            layout,
            scrollY,
            scrollY + viewportHeight,
            viewportWidth.toFloat(),
            zoom,
            bucket,
        )
    }
}

/** Remembers a [NibPdfViewState] for `document`. */
@Composable
public fun rememberNibPdfViewState(document: NibDocument): NibPdfViewState {
    val density = LocalDensity.current.density
    val scope = rememberCoroutineScope()
    return remember(document, density) { NibPdfViewState(document, density, scope) }
}
