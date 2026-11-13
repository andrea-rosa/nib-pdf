package io.github.nibpdf.nib

import androidx.activity.ComponentActivity
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.ui.Modifier
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.compose.ui.test.onAllNodesWithTag
import androidx.compose.ui.test.onRoot
import androidx.compose.ui.test.performTouchInput
import androidx.compose.ui.test.swipeDown
import androidx.compose.ui.test.swipeUp
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import io.github.nibpdf.nib.ui.NibPdfView
import io.github.nibpdf.nib.ui.rememberNibPdfViewState
import kotlinx.coroutines.runBlocking
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith

@RunWith(AndroidJUnit4::class)
class NibPdfViewTest {
    @get:Rule
    val rule = createAndroidComposeRule<ComponentActivity>()

    private fun assetBytes(name: String): ByteArray =
        InstrumentationRegistry
            .getInstrumentation()
            .context.assets
            .open(name)
            .readBytes()

    /**
     * Opens the 100-page fixture, swipes to the end and back, and asserts the
     * last page's tile semantics node exists. No OOM, no exceptions.
     */
    @Test
    fun scrollToEndAndBack() {
        val doc = runBlocking { NibDocument.open(assetBytes("text-100.pdf")) }
        lateinit var state: io.github.nibpdf.nib.ui.NibPdfViewState
        rule.setContent {
            NibPdfView(rememberNibPdfViewState(doc).also { state = it }, Modifier.fillMaxSize())
        }

        repeat(20) {
            rule.onRoot().performTouchInput { swipeUp() }
            rule.waitForIdle()
        }
        // Ensure we actually reached the end even if 20 flings fell short.
        val tag = "tile:99:0:0"
        if (rule.onAllNodesWithTag(tag).fetchSemanticsNodes().isEmpty()) {
            rule.runOnIdle { state.scrollTo(state.maxScrollY()) }
        }
        rule.waitUntil(10_000) {
            rule.onAllNodesWithTag(tag).fetchSemanticsNodes().isNotEmpty()
        }
        assertTrue(rule.onAllNodesWithTag(tag).fetchSemanticsNodes().isNotEmpty())

        repeat(20) {
            rule.onRoot().performTouchInput { swipeDown() }
            rule.waitForIdle()
        }
        rule.runOnIdle { state.scrollTo(0f) }
        rule.waitForIdle()
        doc.close()
    }
}
