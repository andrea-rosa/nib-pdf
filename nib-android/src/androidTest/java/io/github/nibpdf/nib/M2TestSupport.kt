package io.github.nibpdf.nib

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
import io.github.nibpdf.nib.ui.NibPdfView
import io.github.nibpdf.nib.ui.NibPdfViewState
import io.github.nibpdf.nib.ui.Tool
import io.github.nibpdf.nib.ui.rememberNibPdfViewState

internal typealias Rule = AndroidComposeTestRule<ActivityScenarioRule<ComponentActivity>, ComponentActivity>

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
): NibPdfViewState {
    lateinit var state: NibPdfViewState
    rule.setContent {
        NibPdfView(
            rememberNibPdfViewState(doc).also { s -> state = s },
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
