package io.github.nibpdf.nib

import androidx.activity.ComponentActivity
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.test.ext.junit.runners.AndroidJUnit4
import io.github.nibpdf.nib.ui.Tool
import kotlinx.coroutines.runBlocking
import kotlinx.serialization.json.jsonArray
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import kotlin.math.PI
import kotlin.math.sin

@RunWith(AndroidJUnit4::class)
class InkTest {
    @get:Rule
    val rule = createAndroidComposeRule<ComponentActivity>()

    @Test
    fun stylusGestureCreatesInkOp() {
        val doc = runBlocking { NibDocument.open(assetBytes("text.pdf")) }
        val state = setUpView(rule, doc, Tool.Ink)

        // 60 points on a sinusoid, user space x 30→170, 5 periods, y = 60 ± 12.
        val n = 60
        val userPts =
            (0 until n).map { i ->
                val x = 30f + i * (140f / (n - 1))
                val y = 60f + 12f * sin(i / (n - 1f) * 5f * 2f * PI.toFloat())
                Offset(x, y)
            }
        val rootPts = userPts.map { pagePointPx(state, 0, it.x, it.y) }
        val pressures = (0 until n).map { i -> 0.3f + i * (0.5f / (n - 1)) }

        injectStylusStroke(rule, rootPts, pressures)
        waitForAnnotationCount(rule, 1)

        val anns = annotationsOnPage0(state)
        assertEquals(1, anns.size)
        val a = anns[0]
        assertEquals("Ink", a.subtype)
        assertNotNull(a.strokes)
        assertEquals(1, a.strokes!!.size)
        assertTrue("points=${a.strokes!![0].size}", a.strokes!![0].size >= 40)
        a.strokes!![0].forEach { assertEquals(4, it.size) }
        assertEquals(0.3f, a.strokes!![0].first()[2], 0.01f)
        assertEquals(0.8f, a.strokes!![0].last()[2], 0.01f)
        assertEquals("catmullRom", a.smoothingAlgorithm)
        assertEquals(
            "stylus",
            a.json["strokes"]!!
                .jsonArray[0]
                .jsonObject["tool"]!!
                .jsonPrimitive
                .content,
        )
        assertTrue(a.pressureSensitive)
        assertNotNull(a.rect)
        doc.close()
    }
}
