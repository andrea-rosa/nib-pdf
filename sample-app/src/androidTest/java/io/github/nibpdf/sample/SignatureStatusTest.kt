package io.github.nibpdf.sample

import androidx.activity.compose.setContent
import androidx.compose.material3.MaterialTheme
import androidx.compose.ui.test.assertTextEquals
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.compose.ui.test.onAllNodesWithTag
import androidx.compose.ui.test.onNodeWithTag
import androidx.test.ext.junit.runners.AndroidJUnit4
import io.github.nibpdf.nib.delta.SignedDocumentException
import kotlinx.coroutines.runBlocking
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import java.io.ByteArrayOutputStream

@RunWith(AndroidJUnit4::class)
class SignatureStatusTest {
    @get:Rule
    val rule = createAndroidComposeRule<MainActivity>()

    @Test
    fun occupiedFieldBadgeDoesNotClaimIntegrityOrTrust() {
        rule.activityRule.scenario.onActivity { activity ->
            activity.assetIntent = "incr-presigned.pdf"
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
            rule.onAllNodesWithTag("signatureStatusBadge").fetchSemanticsNodes().isNotEmpty()
        }
        rule
            .onNodeWithTag("signatureStatusBadge")
            .assertTextEquals("Signature present. Integrity and trust have not been evaluated.")
    }

    @Test
    fun signedFlattenRefusalLeavesDestinationUntouched() =
        runBlocking {
            val bytes =
                rule.activity.assets
                    .open("incr-presigned.pdf")
                    .use { it.readBytes() }
            val document =
                io.github.nibpdf.nib.NibDocument
                    .open(bytes)
            try {
                val destination = ByteArrayOutputStream().apply { write("existing".toByteArray()) }
                var opened = false
                try {
                    saveAfterValidation(document, optimized = true, flatten = true) {
                        opened = true
                        destination.reset()
                        destination
                    }
                    throw AssertionError("signed flatten should have been rejected")
                } catch (_: SignedDocumentException) {
                    assertFalse(opened)
                    assertTrue(destination.toByteArray().contentEquals("existing".toByteArray()))
                }
            } finally {
                document.close()
            }
        }
}
