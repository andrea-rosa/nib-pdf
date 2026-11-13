package io.github.nibpdf.nib.ui

import android.content.ComponentCallbacks2
import android.content.res.Configuration
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.gestures.Orientation
import androidx.compose.foundation.gestures.ScrollableDefaults
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.gestures.detectTransformGestures
import androidx.compose.foundation.gestures.rememberScrollableState
import androidx.compose.foundation.gestures.scrollable
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.size
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.snapshotFlow
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.graphics.drawscope.scale
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.layout.SubcomposeLayout
import androidx.compose.ui.layout.onSizeChanged
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.unit.Constraints
import androidx.compose.ui.unit.IntOffset
import androidx.compose.ui.unit.IntSize
import io.github.nibpdf.nib.render.ZoomBuckets
import io.github.nibpdf.nib.ui.overlay.AnnotationOverlay
import io.github.nibpdf.nib.ui.overlay.CreateOverlay
import io.github.nibpdf.nib.ui.overlay.InkOverlay
import io.github.nibpdf.nib.ui.overlay.SelectionOverlay
import kotlinx.coroutines.FlowPreview
import kotlinx.coroutines.flow.collectLatest
import kotlinx.coroutines.flow.debounce
import kotlin.math.roundToInt

/**
 * Read-only PDF viewer: vertically stacked pages, fling scroll, pinch zoom
 * (0.5×–8×, zoom buckets of √2), double-tap zoom, 512 px tile rendering via
 * the Rust `RenderWorker`.
 */
@OptIn(FlowPreview::class)
@Composable
@Suppress("ktlint:standard:function-naming")
public fun NibPdfView(
    state: NibPdfViewState,
    modifier: Modifier = Modifier,
) {
    val context = LocalContext.current

    // Forward onTrimMemory to the tile cache.
    DisposableEffect(state) {
        val callbacks =
            object : ComponentCallbacks2 {
                override fun onConfigurationChanged(newConfig: Configuration) {}

                override fun onLowMemory() {
                    state.onTrimMemory(ComponentCallbacks2.TRIM_MEMORY_RUNNING_LOW)
                }

                override fun onTrimMemory(level: Int) {
                    state.onTrimMemory(level)
                }
            }
        context.applicationContext.registerComponentCallbacks(callbacks)
        onDispose { context.applicationContext.unregisterComponentCallbacks(callbacks) }
    }

    // Kick the scheduler whenever the viewport, scroll, or bucket changes.
    // Bucket is discrete so this fires immediately on √2 boundaries (≈1.41
    // ratio, close to the 1.25 settle threshold in the spec); a trailing
    // debounce handles final settle.
    LaunchedEffect(state) {
        snapshotFlow {
            Triple(state.scrollY.roundToInt(), state.bucket, state.viewportHeight)
        }.collect { state.updateScheduler() }
    }
    LaunchedEffect(state) {
        snapshotFlow { state.zoom }
            .debounce(80)
            .collectLatest { state.updateScheduler() }
    }

    val scrollState =
        rememberScrollableState { delta ->
            val old = state.scrollY
            state.scrollTo(old + delta)
            state.scrollY - old
        }

    SubcomposeLayout(
        modifier
            .fillMaxSize()
            .onSizeChanged {
                state.viewportWidth = it.width
                state.viewportHeight = it.height
            }.scrollable(
                scrollState,
                Orientation.Vertical,
                enabled = state.tool == Tool.Pan,
                flingBehavior = ScrollableDefaults.flingBehavior(),
            ).pointerInput(state.tool) {
                if (state.tool != Tool.Pan) return@pointerInput
                detectTransformGestures { centroid, pan, zoomChange, _ ->
                    val oldZoom = state.zoom
                    val newZoom =
                        (oldZoom * zoomChange).coerceIn(
                            ZoomBuckets.MIN_ZOOM,
                            ZoomBuckets.MAX_ZOOM,
                        )
                    // Keep the content point under the gesture centroid fixed.
                    val contentY = state.scrollY + centroid.y - pan.y
                    state.scrollTo(contentY * (newZoom / oldZoom) - centroid.y)
                    state.zoom = newZoom
                }
            }.pointerInput(state.tool) {
                if (state.tool != Tool.Pan) return@pointerInput
                detectTapGestures(
                    onDoubleTap = { offset ->
                        val target =
                            if (state.zoom < 1.5f) 2.5f else 1f
                        val contentY = state.scrollY + offset.y
                        state.scrollTo(contentY * (target / state.zoom) - offset.y)
                        state.zoom = target
                    },
                )
            },
    ) { constraints ->
        val zoom = state.zoom
        val scrollY = state.scrollY
        val visiblePages =
            state.layout.pagesIn(
                scrollY,
                scrollY + constraints.maxHeight,
                zoom,
            )
        val measurables =
            visiblePages.map { page ->
                val w = state.layout.pageWidthPx(page, zoom).roundToInt()
                val h = state.layout.pageHeightPx(page, zoom).roundToInt()
                val c = Constraints.fixed(w.coerceAtLeast(1), h.coerceAtLeast(1))
                page to
                    subcompose("page:$page") {
                        PageItem(state, page)
                    }.map { it.measure(c) }
            }
        layout(constraints.maxWidth, constraints.maxHeight) {
            measurables.forEach { (page, placeables) ->
                val top = (state.layout.pageTop(page, zoom) - scrollY).roundToInt()
                placeables.forEach { p ->
                    val x = ((constraints.maxWidth - p.width) / 2).coerceAtLeast(0)
                    p.placeRelative(x, top)
                }
            }
        }
    }
}

@Composable
@Suppress("ktlint:standard:function-naming")
private fun PageItem(
    state: NibPdfViewState,
    page: Int,
) {
    // Reading the counter subscribes this page to tile arrivals.
    state.tilesChanged.collectAsState().value
    val bucket = state.bucket
    val zoom = state.zoom
    val density = LocalDensity.current.density
    val scale = ZoomBuckets.scaleFor(bucket)
    // content px per rendered device px: page renders at `scale*density` px/pt
    // and is drawn at `zoom*density` px/pt.
    val drawScale = zoom / scale
    val tiles = state.scheduler.tilesForPage(page, bucket)
    // While tiles of the new bucket are still rendering (e.g. mid-pinch),
    // draw the nearest cached bucket scaled underneath so pages never blank.
    val fallback = state.scheduler.fallbackTilesForPage(page, bucket)
    Box(Modifier.fillMaxSize()) {
        Canvas(Modifier.fillMaxSize()) {
            if (fallback != null) {
                val (fbBucket, fbTiles) = fallback
                val fbScale = zoom / ZoomBuckets.scaleFor(fbBucket)
                fbTiles.forEach { (key, bmp) ->
                    scale(fbScale, pivot = Offset.Zero) {
                        drawImage(
                            bmp.asImageBitmap(),
                            srcOffset = IntOffset.Zero,
                            srcSize = IntSize(bmp.width, bmp.height),
                            dstOffset =
                                IntOffset(
                                    key.tx * ZoomBuckets.TILE_SIZE,
                                    key.ty * ZoomBuckets.TILE_SIZE,
                                ),
                            dstSize = IntSize(bmp.width, bmp.height),
                        )
                    }
                }
            }
            tiles.forEach { (key, bmp) ->
                scale(drawScale, pivot = Offset.Zero) {
                    drawImage(
                        bmp.asImageBitmap(),
                        srcOffset = IntOffset.Zero,
                        srcSize = IntSize(bmp.width, bmp.height),
                        dstOffset =
                            IntOffset(
                                key.tx * ZoomBuckets.TILE_SIZE,
                                key.ty * ZoomBuckets.TILE_SIZE,
                            ),
                        dstSize = IntSize(bmp.width, bmp.height),
                    )
                }
            }
        }
        io.github.nibpdf.nib.ui.overlay
            .AnnotationOverlay(state, page)
        when (state.tool) {
            Tool.Ink ->
                io.github.nibpdf.nib.ui.overlay
                    .InkOverlay(state, page)
            Tool.Select ->
                io.github.nibpdf.nib.ui.overlay
                    .SelectionOverlay(state, page)
            Tool.Pan -> Unit
            else ->
                io.github.nibpdf.nib.ui.overlay
                    .CreateOverlay(state, page)
        }
        // Semantics handles for tests: one tagged Box per present tile.
        tiles.keys.forEach { key ->
            val sizePx = ZoomBuckets.TILE_SIZE * drawScale
            with(LocalDensity.current) {
                Box(
                    Modifier
                        .offset {
                            IntOffset(
                                (key.tx * sizePx).roundToInt(),
                                (key.ty * sizePx).roundToInt(),
                            )
                        }.size(sizePx.toDp(), sizePx.toDp())
                        .testTag("tile:${key.page}:${key.tx}:${key.ty}"),
                )
            }
        }
    }
}
