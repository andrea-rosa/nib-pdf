package io.github.nibpdf.nib

import android.graphics.Bitmap
import android.graphics.Canvas
import android.graphics.Color
import android.graphics.PointF
import android.os.SystemClock
import android.view.InputDevice
import android.view.MotionEvent
import androidx.activity.ComponentActivity
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.test.hasContentDescription
import androidx.compose.ui.test.junit4.AndroidComposeTestRule
import androidx.compose.ui.test.onAllNodesWithTag
import androidx.compose.ui.test.onRoot
import androidx.test.ext.junit.rules.ActivityScenarioRule
import androidx.test.platform.app.InstrumentationRegistry
import io.github.nibpdf.nib.render.TileKey
import io.github.nibpdf.nib.render.ZoomBuckets
import io.github.nibpdf.nib.ui.NibPdfView
import io.github.nibpdf.nib.ui.NibPdfViewState
import io.github.nibpdf.nib.ui.PageTransform
import io.github.nibpdf.nib.ui.Tool
import io.github.nibpdf.nib.ui.rememberNibPdfViewState
import io.github.nibpdf.nib.ui.toComposeRect
import kotlin.math.ceil

internal typealias Rule = AndroidComposeTestRule<ActivityScenarioRule<ComponentActivity>, ComponentActivity>

/** Composes the cached tiles of [page] at the current bucket into one page bitmap. */
internal fun composePageBitmap(
    state: NibPdfViewState,
    page: Int,
): Bitmap {
    val tiles = state.scheduler.tilesForPage(page, state.bucket)
    val w = tiles.entries.maxOfOrNull { (k, b) -> k.tx * ZoomBuckets.TILE_SIZE + b.width } ?: 1
    val h = tiles.entries.maxOfOrNull { (k, b) -> k.ty * ZoomBuckets.TILE_SIZE + b.height } ?: 1
    val out = Bitmap.createBitmap(w.coerceAtLeast(1), h.coerceAtLeast(1), Bitmap.Config.ARGB_8888)
    out.eraseColor(Color.WHITE)
    val canvas = Canvas(out)
    tiles.forEach { (k, b) ->
        canvas.drawBitmap(
            b,
            (k.tx * ZoomBuckets.TILE_SIZE).toFloat(),
            (k.ty * ZoomBuckets.TILE_SIZE).toFloat(),
            null,
        )
    }
    return out
}

/** Tile keys of the current bucket intersecting the [page] entries of `document.dirtyLog`. */
internal fun tilesCoveringDirty(
    state: NibPdfViewState,
    page: Int,
): Set<TileKey> {
    val scale = ZoomBuckets.scaleFor(state.bucket)
    val density =
        InstrumentationRegistry
            .getInstrumentation()
            .targetContext.resources.displayMetrics.density
    val info = state.document.pages[page]
    val t = PageTransform(info.cropBox.toComposeRect(), info.rotation, scale * density)
    val maxTx = ((ceil(t.contentSize.width.toDouble()).toInt() - 1) / ZoomBuckets.TILE_SIZE).coerceAtLeast(0)
    val maxTy = ((ceil(t.contentSize.height.toDouble()).toInt() - 1) / ZoomBuckets.TILE_SIZE).coerceAtLeast(0)
    val keys = mutableSetOf<TileKey>()
    for (e in state.document.dirtyLog) {
        if (e.page != page) continue
        val dev = t.rectToContent(e.rect.toComposeRect())
        val tx0 = (dev.left / ZoomBuckets.TILE_SIZE).toInt().coerceIn(0, maxTx)
        val ty0 = (dev.top / ZoomBuckets.TILE_SIZE).toInt().coerceIn(0, maxTy)
        val tx1 = ((dev.right - 1f) / ZoomBuckets.TILE_SIZE).toInt().coerceIn(tx0, maxTx)
        val ty1 = ((dev.bottom - 1f) / ZoomBuckets.TILE_SIZE).toInt().coerceIn(ty0, maxTy)
        for (ty in ty0..ty1) {
            for (tx in tx0..tx1) {
                keys += TileKey(page, state.bucket, tx, ty)
            }
        }
    }
    return keys
}

/** Waits until every tile intersecting a dirty rect of [page] renders at ≥ [revision]. */
internal fun waitTilesAtRevision(
    rule: Rule,
    state: NibPdfViewState,
    page: Int,
    revision: Int,
) {
    val keys = tilesCoveringDirty(state, page)
    rule.waitUntil(30_000) {
        val revs = state.scheduler.tileRevisionsForPage(page, state.bucket)
        keys.isNotEmpty() && keys.all { (revs[it] ?: 0) >= revision }
    }
}

/** Tile keys of the current bucket intersecting a user-space [rect] on [page]. */
internal fun tileKeysCovering(
    state: NibPdfViewState,
    page: Int,
    rect: android.graphics.RectF,
): Set<TileKey> {
    val scale = ZoomBuckets.scaleFor(state.bucket)
    val density =
        InstrumentationRegistry
            .getInstrumentation()
            .targetContext.resources.displayMetrics.density
    val info = state.document.pages[page]
    val t = PageTransform(info.cropBox.toComposeRect(), info.rotation, scale * density)
    val maxTx = ((ceil(t.contentSize.width.toDouble()).toInt() - 1) / ZoomBuckets.TILE_SIZE).coerceAtLeast(0)
    val maxTy = ((ceil(t.contentSize.height.toDouble()).toInt() - 1) / ZoomBuckets.TILE_SIZE).coerceAtLeast(0)
    val dev = t.rectToContent(rect.toComposeRect())
    val tx0 = (dev.left / ZoomBuckets.TILE_SIZE).toInt().coerceIn(0, maxTx)
    val ty0 = (dev.top / ZoomBuckets.TILE_SIZE).toInt().coerceIn(0, maxTy)
    val tx1 = ((dev.right - 1f) / ZoomBuckets.TILE_SIZE).toInt().coerceIn(tx0, maxTx)
    val ty1 = ((dev.bottom - 1f) / ZoomBuckets.TILE_SIZE).toInt().coerceIn(ty0, maxTy)
    val keys = mutableSetOf<TileKey>()
    for (ty in ty0..ty1) {
        for (tx in tx0..tx1) {
            keys += TileKey(page, state.bucket, tx, ty)
        }
    }
    return keys
}

/** Crops a page-bucket [bitmap] to a user-space [rect] on [page] (content px). */
internal fun cropUser(
    bitmap: Bitmap,
    state: NibPdfViewState,
    page: Int,
    rect: android.graphics.RectF,
): Bitmap {
    val scale = ZoomBuckets.scaleFor(state.bucket)
    val density =
        InstrumentationRegistry
            .getInstrumentation()
            .targetContext.resources.displayMetrics.density
    val info = state.document.pages[page]
    val t = PageTransform(info.cropBox.toComposeRect(), info.rotation, scale * density)
    val c = t.rectToContent(rect.toComposeRect())
    val x = c.left.toInt().coerceIn(0, bitmap.width - 1)
    val y = c.top.toInt().coerceIn(0, bitmap.height - 1)
    val w =
        c.width
            .toInt()
            .coerceAtLeast(1)
            .coerceAtMost(bitmap.width - x)
    val h =
        c.height
            .toInt()
            .coerceAtLeast(1)
            .coerceAtMost(bitmap.height - y)
    return Bitmap.createBitmap(bitmap, x, y, w, h)
}

internal fun assetBytes(name: String): ByteArray =
    InstrumentationRegistry
        .getInstrumentation()
        .context.assets
        .open(name)
        .readBytes()

/** Sets content to a full-screen [NibPdfView] on [tool]; waits for the first tile. */
internal fun setUpView(
    rule: Rule,
    doc: NibDocument,
    tool: Tool,
    flushPolicy: FlushPolicy = FlushPolicy(enabled = false),
): NibPdfViewState {
    lateinit var state: NibPdfViewState
    rule.setContent {
        NibPdfView(
            rememberNibPdfViewState(doc, flushPolicy).also { s -> state = s },
            Modifier.fillMaxSize(),
        )
    }
    state.tool = tool
    rule.waitForIdle()
    rule.waitUntil(10_000) {
        rule
            .onAllNodesWithTag("tile:0:0:0", useUnmergedTree = true)
            .fetchSemanticsNodes()
            .isNotEmpty()
    }
    return state
}

/** Maps a user-space point on [page] to root-layout px coordinates. */
internal fun pagePointPx(
    state: NibPdfViewState,
    page: Int,
    xPt: Float,
    yPt: Float,
): Offset = state.pageOriginInView(page) + state.transform(page).toContent(Offset(xPt, yPt))

internal fun annotationCount(rule: Rule): Int =
    rule
        .onAllNodes(hasContentDescription("annotation:", substring = true), useUnmergedTree = true)
        .fetchSemanticsNodes()
        .size

internal fun waitForAnnotationCount(
    rule: Rule,
    n: Int,
) {
    rule.waitUntil(5_000) { annotationCount(rule) == n }
}

internal fun annotationsOnPage0(state: NibPdfViewState) = state.document.annotations(0)

/**
 * Dispatches a stylus stroke through the activity's decorView: DOWN, one
 * MOVE per point (8 ms apart), UP. [points] are root-layout px; the
 * decorView's window offset is compensated via the root semantics node.
 */
internal fun injectStylusStroke(
    rule: Rule,
    points: List<Offset>,
    pressures: List<Float>,
) {
    require(points.isNotEmpty())
    val root = rule.onRoot().fetchSemanticsNode().positionInWindow
    val downTime = SystemClock.uptimeMillis()
    val events = mutableListOf<MotionEvent>()

    fun event(
        action: Int,
        index: Int,
        eventTime: Long,
    ): MotionEvent {
        val props =
            arrayOf(
                MotionEvent.PointerProperties().apply {
                    id = 0
                    toolType = MotionEvent.TOOL_TYPE_STYLUS
                },
            )
        val coords =
            arrayOf(
                MotionEvent.PointerCoords().apply {
                    x = points[index].x + root.x
                    y = points[index].y + root.y
                    pressure = pressures.getOrElse(index) { 0.5f }
                    size = 1f
                },
            )
        return MotionEvent.obtain(
            downTime,
            eventTime,
            action,
            1,
            props,
            coords,
            0,
            0,
            1f,
            1f,
            0,
            0,
            InputDevice.SOURCE_STYLUS,
            0,
        )
    }

    events += event(MotionEvent.ACTION_DOWN, 0, downTime)
    points.forEachIndexed { i, _ ->
        if (i > 0) events += event(MotionEvent.ACTION_MOVE, i, downTime + i * 8L)
    }
    events += event(MotionEvent.ACTION_UP, points.lastIndex, downTime + points.size * 8L)

    rule.runOnUiThread {
        val decor = rule.activity.window.decorView
        events.forEach { decor.dispatchTouchEvent(it) }
    }
    events.forEach { it.recycle() }
    rule.waitForIdle()
}

internal fun userPoint(
    state: NibPdfViewState,
    page: Int,
    rootPx: Offset,
): PointF {
    val t = state.transform(page)
    val origin = state.pageOriginInView(page)
    val u = t.toUser(rootPx - origin)
    return PointF(u.x, u.y)
}
