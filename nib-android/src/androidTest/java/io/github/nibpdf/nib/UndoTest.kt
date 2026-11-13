package io.github.nibpdf.nib

import androidx.activity.ComponentActivity
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.compose.ui.test.onRoot
import androidx.compose.ui.test.performTouchInput
import androidx.compose.ui.test.click
import androidx.compose.ui.test.swipe
import androidx.test.ext.junit.runners.AndroidJUnit4
import io.github.nibpdf.nib.ui.Tool
import kotlinx.coroutines.runBlocking
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import kotlin.math.PI
import kotlin.math.sin

@RunWith(AndroidJUnit4::class)
class UndoTest {
    @get:Rule
    val rule = createAndroidComposeRule<ComponentActivity>()

    @Test
    fun drawHighlightMoveUndoRedo() {
        val doc = runBlocking { NibDocument.open(assetBytes("text.pdf")) }
        val state = setUpView(rule, doc, Tool.Ink)

        // Ink gesture: short sinusoid around y=60.
        val n = 30
        val pts =
            (0 until n).map { i ->
                val x = 30f + i * (140f / (n - 1))
                Offset(x, 60f + 12f * sin(i / (n - 1f) * 2f * PI.toFloat()))
            }
        injectStylusStroke(
            rule,
            pts.map { pagePointPx(state, 0, it.x, it.y) },
            (0 until n).map { 0.5f },
        )
        waitForAnnotationCount(rule, 1)

        // Highlight gesture over the top two text lines.
        rule.runOnIdle { state.tool = Tool.Highlight }
        rule.onRoot().performTouchInput {
            swipe(
                start = pagePointPx(state, 0, 45f, 168f),
                end = pagePointPx(state, 0, 150f, 138f),
                durationMillis = 300,
            )
        }
        waitForAnnotationCount(rule, 2)

        // Select the ink annotation (bottom one) and drag it by +40/+40 px.
        rule.runOnIdle { state.tool = Tool.Select }
        val ink = annotationsOnPage0(state).first { it.subtype == "Ink" }
        val rectBefore = ink.rect!!.let { android.graphics.RectF(it) }
        // Click a point that lies ON the stroke polyline (hit-test measures
        // distance to the stroke, not the bbox).
        val onStroke = pts[n / 2]
        val centrePx = pagePointPx(state, 0, onStroke.x, onStroke.y)
        rule.onRoot().performTouchInput { click(centrePx) }
        rule.waitForIdle()
        assertEquals(ink.id, state.selectedId)

        rule.onRoot().performTouchInput {
            swipe(start = centrePx, end = centrePx + Offset(40f, 40f), durationMillis = 300)
        }
        rule.waitForIdle()
        assertEquals(2, annotationCount(rule))
        val inkAfter = annotationsOnPage0(state).first { it.id == ink.id }
        val rectMoved = inkAfter.rect!!
        assertNotEquals(rectBefore.left, rectMoved.left)

        // Undo ×3: move → highlight → ink.
        rule.runOnIdle { doc.undo() }
        waitForAnnotationCount(rule, 2)
        val inkRestored = annotationsOnPage0(state).first { it.id == ink.id }
        assertEquals(rectBefore.left, inkRestored.rect!!.left, 0.5f)
        rule.runOnIdle { doc.undo() }
        waitForAnnotationCount(rule, 1)
        rule.runOnIdle { doc.undo() }
        waitForAnnotationCount(rule, 0)

        // Redo ×3.
        rule.runOnIdle { doc.redo() }
        waitForAnnotationCount(rule, 1)
        rule.runOnIdle { doc.redo() }
        waitForAnnotationCount(rule, 2)
        rule.runOnIdle { doc.redo() }
        rule.waitForIdle()
        assertEquals(2, annotationCount(rule))
        val inkRedone = annotationsOnPage0(state).first { it.id == ink.id }
        assertEquals(rectMoved.left, inkRedone.rect!!.left, 0.5f)
        assertTrue(inkRedone.rect!!.left != rectBefore.left)
        doc.close()
    }
}
