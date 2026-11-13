package io.github.nibpdf.nib

import androidx.activity.ComponentActivity
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.compose.ui.test.onRoot
import androidx.compose.ui.test.performTouchInput
import androidx.compose.ui.test.swipe
import androidx.test.ext.junit.runners.AndroidJUnit4
import io.github.nibpdf.nib.ui.Tool
import kotlinx.coroutines.runBlocking
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import kotlin.math.abs

@RunWith(AndroidJUnit4::class)
class HighlightSnapTest {
    @get:Rule
    val rule = createAndroidComposeRule<ComponentActivity>()

    @Test
    fun dragAcrossTwoLinesSnapsTwoQuads() {
        val doc = runBlocking { NibDocument.open(assetBytes("text.pdf")) }
        val state = setUpView(rule, doc, Tool.Highlight)

        // Char centres on the two top lines (baselines y=160, y=144); the
        // rect must include both lines' centres but exclude y=128's.
        rule.onRoot().performTouchInput {
            swipe(
                start = pagePointPx(state, 0, 45f, 168f),
                end = pagePointPx(state, 0, 150f, 138f),
                durationMillis = 300,
            )
        }
        waitForAnnotationCount(rule, 1)

        val a = annotationsOnPage0(state).single()
        assertEquals("Highlight", a.subtype)
        val quads = a.quadPoints!!
        assertEquals(2, quads.size)
        // Stored UL, UR, LL, LR → height = y(UL) − y(LL).
        val heights = quads.map { q -> q[1] - q[5] }
        assertTrue(abs(heights[0] - heights[1]) <= 1f)
        heights.forEach { assertTrue("h=$it", it in 8f..16f) }
        // Tops (y of UL corner) differ by one 16 pt line pitch.
        assertTrue("tops ${quads[0][1]} vs ${quads[1][1]}", abs(quads[0][1] - quads[1][1]) in 15f..17f)

        val q0 = quads[0]
        val bbox =
            android.graphics.RectF(
                minOf(q0[0], q0[2]),
                minOf(q0[1], q0[5]),
                maxOf(q0[0], q0[2]),
                maxOf(q0[1], q0[5]),
            )
        val text = doc.textInRect(0, bbox)
        assertTrue("text=$text", text.contains("quick brown"))
        doc.close()
    }
}
