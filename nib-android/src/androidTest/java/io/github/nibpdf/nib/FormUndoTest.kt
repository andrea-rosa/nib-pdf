package io.github.nibpdf.nib

import androidx.activity.ComponentActivity
import androidx.compose.ui.test.hasSetTextAction
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performTextInput
import androidx.test.ext.junit.runners.AndroidJUnit4
import io.github.nibpdf.nib.ui.Tool
import kotlinx.coroutines.runBlocking
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith

/**
 * `update` form-field ops participate in undo/redo like any other delta:
 * typed text, checkbox toggles and radio picks are all reverted in order.
 */
@RunWith(AndroidJUnit4::class)
class FormUndoTest {
    @get:Rule
    val rule = createAndroidComposeRule<ComponentActivity>()

    @Test
    fun undoRedoAcrossFormEdits() {
        val doc = runBlocking { NibDocument.open(assetBytes("form.pdf")) }
        setUpView(rule, doc, Tool.Form)

        val nameBefore = doc.formField("name")!!.valueString
        val agreeBefore = doc.formField("agree")!!.valueString
        val colorBefore = doc.formField("color")!!.valueString

        rule.onNodeWithTag("field:name", useUnmergedTree = true).performClick()
        rule.waitUntil(5_000) {
            rule
                .onAllNodes(hasSetTextAction(), useUnmergedTree = true)
                .fetchSemanticsNodes()
                .isNotEmpty()
        }
        rule.onNode(hasSetTextAction(), useUnmergedTree = true).performTextInput("ab")
        rule.onNodeWithTag("field:agree", useUnmergedTree = true).performClick()
        rule.onNodeWithTag("field:color:Blue", useUnmergedTree = true).performClick()

        assertEquals("ab", doc.formField("name")!!.valueString)
        assertEquals("Yes", doc.formField("agree")!!.valueString)
        assertEquals("Blue", doc.formField("color")!!.valueString)
        assertTrue(doc.undoAvailable.value)

        rule.runOnIdle { doc.undo() }
        assertEquals(colorBefore, doc.formField("color")!!.valueString)
        rule.runOnIdle { doc.undo() }
        assertEquals(agreeBefore, doc.formField("agree")!!.valueString)
        rule.runOnIdle { doc.undo() }
        assertEquals(nameBefore, doc.formField("name")!!.valueString)

        assertFalse(doc.undoAvailable.value)
        assertTrue(doc.redoAvailable.value)

        rule.runOnIdle { doc.redo() }
        assertEquals("ab", doc.formField("name")!!.valueString)
        rule.runOnIdle { doc.redo() }
        assertEquals("Yes", doc.formField("agree")!!.valueString)
        rule.runOnIdle { doc.redo() }
        assertEquals("Blue", doc.formField("color")!!.valueString)

        assertTrue(doc.undoAvailable.value)
        assertFalse(doc.redoAvailable.value)
    }
}
