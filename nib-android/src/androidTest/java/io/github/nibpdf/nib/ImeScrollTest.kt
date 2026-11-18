package io.github.nibpdf.nib

import androidx.activity.ComponentActivity
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.performClick
import androidx.test.ext.junit.runners.AndroidJUnit4
import io.github.nibpdf.nib.ui.Tool
import io.github.nibpdf.nib.ui.toComposeRect
import kotlinx.coroutines.runBlocking
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith

/**
 * Focusing a widget near the viewport bottom scrolls it above the IME
 * inset (here simulated via `imeBottomOverridePx`).
 */
@RunWith(AndroidJUnit4::class)
class ImeScrollTest {
    @get:Rule
    val rule = createAndroidComposeRule<ComponentActivity>()

    @Test
    fun focusNearBottomScrollsAboveIme() {
        val doc = runBlocking { NibDocument.open(assetBytes("form.pdf")) }
        val state = setUpView(rule, doc, Tool.Form)

        rule.runOnIdle {
            state.imeBottomOverridePx = 700
            state.scrollTo(state.maxScrollY())
        }
        rule.waitForIdle()

        rule.onNodeWithTag("field:bottom.note", useUnmergedTree = true).performClick()
        rule.waitUntil(5_000) { state.focusedField == "bottom.note" }
        rule.waitForIdle()

        val rect =
            doc
                .formField("bottom.note")!!
                .widgets
                .first { it.page == 0 }
                .rect
        val contentBottom =
            state.pageOriginInView(0).y +
                state.transform(0).rectToContent(rect.toComposeRect()).bottom
        assertTrue(
            "widget bottom $contentBottom not above ime (viewportHeight=${state.viewportHeight})",
            contentBottom <= state.viewportHeight - 700f + 2f,
        )
    }
}
