package io.github.nibpdf.nib

import androidx.activity.ComponentActivity
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onRoot
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performTouchInput
import androidx.compose.ui.test.click
import androidx.compose.ui.test.swipe
import androidx.test.ext.junit.runners.AndroidJUnit4
import io.github.nibpdf.nib.ui.Tool
import kotlinx.coroutines.runBlocking
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith

@RunWith(AndroidJUnit4::class)
class SelectionTest {
    @get:Rule
    val rule = createAndroidComposeRule<ComponentActivity>()

    @Test
    fun deleteSelectedSquare() {
        val doc = runBlocking { NibDocument.open(assetBytes("text.pdf")) }
        val state = setUpView(rule, doc, Tool.Square)

        rule.onRoot().performTouchInput {
            swipe(
                start = pagePointPx(state, 0, 50f, 90f),
                end = pagePointPx(state, 0, 90f, 50f),
                durationMillis = 200,
            )
        }
        waitForAnnotationCount(rule, 1)

        rule.runOnIdle { state.tool = Tool.Select }
        rule.onRoot().performTouchInput {
            click(pagePointPx(state, 0, 70f, 70f))
        }
        rule.waitForIdle()
        assertNotNull(state.selectedId)

        rule.onNodeWithTag("deleteSelected", useUnmergedTree = true).performClick()
        waitForAnnotationCount(rule, 0)
        assertNull(state.selectedId)

        rule.runOnIdle { doc.undo() }
        waitForAnnotationCount(rule, 1)
        doc.close()
    }
}
