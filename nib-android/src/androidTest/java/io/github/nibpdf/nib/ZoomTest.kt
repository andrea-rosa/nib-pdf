package io.github.nibpdf.nib

import androidx.activity.ComponentActivity
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.compose.ui.test.onRoot
import androidx.compose.ui.test.performTouchInput
import androidx.compose.ui.test.pinch
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import io.github.nibpdf.nib.ui.NibPdfView
import io.github.nibpdf.nib.ui.rememberNibPdfViewState
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.runBlocking
import kotlinx.coroutines.withTimeout
import org.junit.Assert.assertEquals
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith

@RunWith(AndroidJUnit4::class)
class ZoomTest {
    @get:Rule
    val rule = createAndroidComposeRule<ComponentActivity>()

    private fun assetBytes(name: String): ByteArray =
        InstrumentationRegistry.getInstrumentation().context.assets.open(name).readBytes()

    /**
     * Pinch out 4×: bucket becomes 4 and a bucket-4 tile is requested within
     * 300 ms of the gesture ending.
     */
    @Test
    fun pinchTo4xRequestsBucket4() {
        val doc = runBlocking { NibDocument.open(assetBytes("text.pdf")) }
        lateinit var state: io.github.nibpdf.nib.ui.NibPdfViewState
        rule.setContent {
            NibPdfView(rememberNibPdfViewState(doc).also { state = it }, Modifier.fillMaxSize())
        }
        rule.waitForIdle()

        val bounds = rule.onRoot().fetchSemanticsNode().boundsInRoot
        val cx = bounds.center.x
        val cy = bounds.center.y
        // Distance ratio 4 → zoom 4.
        rule.onRoot().performTouchInput {
            pinch(
                Offset(cx, cy - 100f),
                Offset(cx, cy + 100f),
                Offset(cx, cy - 400f),
                Offset(cx, cy + 400f),
            )
        }
        rule.waitUntil(5_000) { state.bucket == 4 }
        assertEquals(4, state.bucket)

        val key =
            runBlocking {
                withTimeout(300) { state.tileRequests.first { it.bucket == 4 } }
            }
        assertEquals(4, key.bucket)
        doc.close()
    }
}
