package io.github.nibpdf.sample

import androidx.activity.compose.setContent
import androidx.compose.material3.MaterialTheme
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.test.assertIsEnabled
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.compose.ui.test.onAllNodesWithTag
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performScrollTo
import androidx.compose.ui.test.performTextInput
import androidx.compose.ui.test.performTouchInput
import androidx.test.ext.junit.runners.AndroidJUnit4
import io.github.nibpdf.nib.NibDocument
import io.github.nibpdf.nib.delta.SignatureFieldState
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import java.util.concurrent.atomic.AtomicReference

@RunWith(AndroidJUnit4::class)
class SampleVisualSignatureTest {
    @get:Rule
    val rule = createAndroidComposeRule<MainActivity>()

    @Test
    fun fixtureWidgetOpensRealCaptureSheetAndReusesMark() {
        rule.activityRule.scenario.onActivity { activity ->
            activity.assetIntent = "sig-field.pdf"
            activity.setContent {
                MaterialTheme {
                    SampleScreen(
                        openAsset = { name -> activity.assets.open(name).use { it.readBytes() } },
                        openUri = { null },
                        decodeStamp = { null },
                        onShare = {},
                    )
                }
            }
        }
        rule.waitUntil(30_000) {
            rule.onAllNodesWithTag("tool:Form", useUnmergedTree = true).fetchSemanticsNodes().isNotEmpty()
        }
        rule.onNodeWithTag("tool:Form", useUnmergedTree = true).performScrollTo().performClick()
        rule.onNodeWithTag("field:Signature1", useUnmergedTree = true).performClick()
        rule.onNodeWithTag("signature:mode:type").performClick()
        rule.onNodeWithTag("signature:text").performScrollTo().performTextInput("Ada")
        rule.onNodeWithTag("signature:apply").performClick()
        rule
            .onNodeWithTag("undo", useUnmergedTree = true)
            .performScrollTo()
            .assertIsEnabled()
            .performClick()
        rule.onNodeWithTag("field:Signature1", useUnmergedTree = true).performClick()
        rule.onNodeWithTag("signature:reuse").performScrollTo().performClick()
        rule.onNodeWithTag("undo", useUnmergedTree = true).performScrollTo().assertIsEnabled()
    }

    @Test
    fun drawingInCaptureSheetCreatesLockedInkAtWidgetRect() {
        val opened = AtomicReference<NibDocument>()
        rule.activityRule.scenario.onActivity { activity ->
            activity.assetIntent = "sig-field.pdf"
            activity.setContent {
                MaterialTheme {
                    SampleScreen(
                        openAsset = { name -> activity.assets.open(name).use { it.readBytes() } },
                        openUri = { null },
                        decodeStamp = { null },
                        onShare = {},
                        openDocument = { bytes -> NibDocument.open(bytes).also(opened::set) },
                    )
                }
            }
        }
        rule.waitUntil(30_000) {
            rule.onAllNodesWithTag("tool:Form", useUnmergedTree = true).fetchSemanticsNodes().isNotEmpty()
        }
        rule.onNodeWithTag("tool:Form", useUnmergedTree = true).performScrollTo().performClick()
        rule.onNodeWithTag("field:Signature1", useUnmergedTree = true).performClick()
        rule.onNodeWithTag("signature:canvas").performScrollTo().performTouchInput {
            down(Offset(20f, 20f))
            moveTo(Offset(120f, 50f))
            up()
        }
        rule.onNodeWithTag("signature:apply").performClick()
        rule.runOnIdle {
            val document = requireNotNull(opened.get())
            val field = requireNotNull(document.formField("Signature1"))
            val annotation = document.annotations(0).single { it.subtype == "Ink" }
            assertEquals(field.widgets.single().rect, annotation.rect)
            assertTrue(annotation.flagsLocked)
            assertTrue(requireNotNull(annotation.strokes).single().size >= 2)
            assertEquals(SignatureFieldState.Empty, field.signatureState)
        }
    }
}
