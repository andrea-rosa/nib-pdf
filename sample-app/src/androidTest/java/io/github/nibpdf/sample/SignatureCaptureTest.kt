package io.github.nibpdf.sample

import android.graphics.Bitmap
import androidx.activity.ComponentActivity
import androidx.compose.material3.MaterialTheme
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.test.assertIsNotEnabled
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performScrollTo
import androidx.compose.ui.test.performTextInput
import androidx.compose.ui.test.performTouchInput
import androidx.test.ext.junit.runners.AndroidJUnit4
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith

@RunWith(AndroidJUnit4::class)
class SignatureCaptureTest {
    @get:Rule
    val rule = createAndroidComposeRule<ComponentActivity>()

    @Test
    fun drawCancelAndReuseAreSessionOnly() {
        var visible by mutableStateOf(true)
        var previous by mutableStateOf<CapturedSignature?>(null)
        var captured: CapturedSignature? = null
        var applied = 0
        rule.setContent {
            MaterialTheme {
                if (visible) {
                    SignatureCaptureDialog(
                        previous = previous,
                        pickedImage = null,
                        onPickImage = {},
                        onDismiss = { visible = false },
                        onCaptureError = { throw AssertionError(it) },
                        onApply = {
                            captured = it
                            previous = it
                            applied++
                            visible = false
                        },
                    )
                }
            }
        }
        rule.onNodeWithTag("signature:apply").assertIsNotEnabled()
        rule.onNodeWithTag("signature:canvas").performScrollTo().performTouchInput {
            down(Offset(20f, 20f))
            moveTo(Offset(120f, 50f))
            up()
        }
        rule.onNodeWithTag("signature:cancel").performClick()
        rule.runOnIdle {
            assertNull(captured)
            assertEquals(0, applied)
            visible = true
        }
        rule.onNodeWithTag("signature:reuse").assertDoesNotExist()
        rule.onNodeWithTag("signature:canvas").performScrollTo().performTouchInput {
            down(Offset(15f, 20f))
            moveTo(Offset(140f, 60f))
            up()
        }
        rule.onNodeWithTag("signature:apply").performClick()
        rule.runOnIdle {
            assertTrue(captured?.mark is SignatureMark.Ink)
            assertEquals(1, applied)
            visible = true
        }
        rule.onNodeWithTag("signature:reuse").performScrollTo().performClick()
        rule.runOnIdle { assertEquals(2, applied) }
    }

    @Test
    fun typeAndPickedImageProduceImageMarks() {
        var visible by mutableStateOf(true)
        var picked by mutableStateOf<Bitmap?>(null)
        var captured: CapturedSignature? = null
        val image = Bitmap.createBitmap(64, 32, Bitmap.Config.ARGB_8888)
        rule.setContent {
            MaterialTheme {
                if (visible) {
                    SignatureCaptureDialog(
                        previous = null,
                        pickedImage = picked,
                        onPickImage = { picked = image },
                        onDismiss = { visible = false },
                        onCaptureError = { throw AssertionError(it) },
                        onApply = {
                            captured = it
                            visible = false
                        },
                    )
                }
            }
        }
        rule.onNodeWithTag("signature:mode:type").performClick()
        rule.onNodeWithTag("signature:text").performScrollTo().performTextInput("Ada Example")
        rule.onNodeWithTag("signature:apply").performClick()
        rule.runOnIdle {
            val mark = requireNotNull(captured)
            assertEquals("Ada Example", mark.signerName)
            assertTrue(mark.mark is SignatureMark.Image)
            assertTrue((mark.mark as SignatureMark.Image).png.isNotEmpty())
            captured = null
            visible = true
        }
        rule.onNodeWithTag("signature:mode:image").performClick()
        rule.onNodeWithTag("signature:apply").assertIsNotEnabled()
        rule.onNodeWithTag("signature:pickImage").performScrollTo().performClick()
        rule.onNodeWithTag("signature:imagePreview").performScrollTo()
        rule.onNodeWithTag("signature:apply").performClick()
        rule.runOnIdle {
            val mark = requireNotNull(captured).mark as SignatureMark.Image
            assertEquals(64, mark.width)
            assertEquals(32, mark.height)
            assertTrue(mark.png.isNotEmpty())
        }
    }
}
