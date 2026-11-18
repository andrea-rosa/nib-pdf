package io.github.nibpdf.nib

import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.ui.Modifier
import androidx.compose.ui.test.hasSetTextAction
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performTextInput
import androidx.test.ext.junit.runners.AndroidJUnit4
import io.github.nibpdf.nib.ui.NibPdfView
import io.github.nibpdf.nib.ui.NibPdfViewState
import io.github.nibpdf.nib.ui.Tool
import io.github.nibpdf.nib.ui.rememberNibPdfViewState
import kotlinx.coroutines.runBlocking
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import kotlin.concurrent.thread

/**
 * Form-tool end to end: fill fields in the overlay, flush, verify the
 * values are baked into the tile bytes and survive a save/reopen.
 */
@RunWith(AndroidJUnit4::class)
class FormFillTest {
    @get:Rule
    val rule = createAndroidComposeRule<ComponentActivity>()

    @Test
    fun fillFlushSaveReopen() {
        val doc = runBlocking { NibDocument.open(assetBytes("form.pdf")) }
        val state = setUpView(rule, doc, Tool.Form)

        val nameRect =
            doc
                .formField("name")!!
                .widgets
                .first { it.page == 0 }
                .rect
        // Blank baseline of the name widget rect before any edits.
        val cropBlank = cropUser(composePageBitmap(state, 0), state, 0, nameRect)

        rule.onNodeWithTag("field:name", useUnmergedTree = true).performClick()
        // The transparent hit-target reveals a BasicTextField once focused.
        rule.waitUntil(5_000) {
            rule
                .onAllNodes(hasSetTextAction(), useUnmergedTree = true)
                .fetchSemanticsNodes()
                .isNotEmpty()
        }
        rule.onNode(hasSetTextAction(), useUnmergedTree = true).performTextInput("Andrea")
        assertEquals("Andrea", doc.formField("name")!!.valueString)

        rule.onNodeWithTag("field:agree", useUnmergedTree = true).performClick()
        assertEquals("Yes", doc.formField("agree")!!.valueString)

        rule.onNodeWithTag("field:color:Green", useUnmergedTree = true).performClick()
        assertEquals("Green", doc.formField("color")!!.valueString)

        rule.onNodeWithTag("field:country", useUnmergedTree = true).performClick()
        rule.onNodeWithText("Italy").performClick()
        assertEquals("IT", doc.formField("country")!!.valueString)

        var revision = -1
        thread { revision = doc.flush().revision }.join()
        assertEquals(1, revision)

        // Wait until the tiles covering the flushed widgets carry rev 1.
        waitTilesAtRevision(rule, state, 0, 1)
        rule.waitForIdle()

        val bytesA = doc.bytes()
        val cropA = cropUser(composePageBitmap(state, 0), state, 0, nameRect)

        val docB = runBlocking { NibDocument.open(bytesA) }
        assertEquals("Andrea", docB.formField("name")!!.valueString)
        assertEquals("Yes", docB.formField("agree")!!.valueString)
        assertEquals("Green", docB.formField("color")!!.valueString)
        assertEquals("IT", docB.formField("country")!!.valueString)

        // Second view on the same activity (Pan mode): `rule.setContent`
        // is single-use, so populate the ComposeView directly.
        lateinit var stateB: NibPdfViewState
        rule.activityRule.scenario.onActivity { activity ->
            activity.setContent {
                NibPdfView(
                    rememberNibPdfViewState(docB, FlushPolicy(enabled = false))
                        .also { s -> stateB = s },
                    Modifier.fillMaxSize(),
                )
            }
        }
        rule.waitForIdle()
        rule.runOnIdle { stateB.tool = Tool.Pan }
        val keys = tileKeysCovering(stateB, 0, nameRect)
        rule.waitUntil(30_000) {
            keys.isNotEmpty() &&
                keys.all { stateB.scheduler.tilesForPage(0, stateB.bucket).containsKey(it) }
        }
        rule.waitForIdle()
        val cropB = cropUser(composePageBitmap(stateB, 0), stateB, 0, nameRect)

        val dAB = PHash.distance(cropA, cropB)
        val dBlank = PHash.distance(cropB, cropBlank)
        android.util.Log.i("NIBTEST", "FormFillTest pHash: cropA-vs-cropB=$dAB cropB-vs-blank=$dBlank")
        assertTrue("flushed vs reopened render differs: pHash=$dAB", dAB <= 6)
        assertTrue("reopened render matches the blank fixture: pHash=$dBlank", dBlank > 6)
    }
}
