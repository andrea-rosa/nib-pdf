package io.github.nibpdf.nib

import android.graphics.Bitmap
import android.graphics.Color
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.test.click
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.compose.ui.test.onRoot
import androidx.compose.ui.test.performTouchInput
import androidx.compose.ui.test.swipe
import androidx.test.ext.junit.runners.AndroidJUnit4
import io.github.nibpdf.nib.delta.DeltaDocument
import io.github.nibpdf.nib.delta.NibJson
import io.github.nibpdf.nib.ui.NibPdfView
import io.github.nibpdf.nib.ui.NibPdfViewState
import io.github.nibpdf.nib.ui.StampSpec
import io.github.nibpdf.nib.ui.Tool
import io.github.nibpdf.nib.ui.rememberNibPdfViewState
import kotlinx.coroutines.runBlocking
import kotlinx.serialization.decodeFromString
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import java.io.ByteArrayOutputStream
import kotlin.math.PI
import kotlin.math.sin

/**
 * Create → flush → save → reopen: annotations persist into the PDF bytes
 * and a reopened document renders the same page.
 */
@RunWith(AndroidJUnit4::class)
class PersistenceTest {
    @get:Rule
    val rule = createAndroidComposeRule<ComponentActivity>()

    @Test
    fun createFlushSaveReopen() {
        val doc = runBlocking { NibDocument.open(assetBytes("text.pdf")) }
        val state = setUpView(rule, doc, Tool.Ink)

        // Ink stroke: short sinusoid.
        val n = 30
        val pts =
            (0 until n).map { i ->
                val x = 30f + i * (140f / (n - 1))
                Offset(x, 60f + 12f * sin(i / (n - 1f) * 2f * PI.toFloat()))
            }
        injectStylusStroke(
            rule,
            pts.map { pagePointPx(state, 0, it.x, it.y) },
            (0 until n).map { 0.5f },
        )
        waitForAnnotationCount(rule, 1)

        // Highlight over the top two text lines.
        rule.runOnIdle { state.tool = Tool.Highlight }
        rule.onRoot().performTouchInput {
            swipe(
                start = pagePointPx(state, 0, 45f, 168f),
                end = pagePointPx(state, 0, 150f, 138f),
                durationMillis = 300,
            )
        }
        waitForAnnotationCount(rule, 2)

        // Image stamp: tiny red PNG placed by a tap.
        val png =
            ByteArrayOutputStream()
                .also { out ->
                    Bitmap
                        .createBitmap(16, 16, Bitmap.Config.ARGB_8888)
                        .apply { eraseColor(Color.RED) }
                        .compress(Bitmap.CompressFormat.PNG, 100, out)
                }.toByteArray()
        rule.runOnIdle {
            state.pendingStamp = StampSpec.Image(png, 16, 16)
            state.tool = Tool.Stamp
        }
        rule.waitForIdle()
        rule.onRoot().performTouchInput { click(pagePointPx(state, 0, 150f, 30f)) }
        waitForAnnotationCount(rule, 3)
        assertEquals(3, doc.uncommittedCount)

        // Flush, then wait for the dirty tiles to re-render at revision 1.
        val report = runBlocking { doc.flush() }
        assertEquals(1, report.revision)
        waitTilesAtRevision(rule, state, page = 0, revision = 1)
        rule.waitForIdle()
        val beforeKeys = state.scheduler.tilesForPage(0, state.bucket).keys
        val before = composePageBitmap(state, 0)

        val bytes =
            ByteArrayOutputStream()
                .also { out -> runBlocking { doc.save(out) } }
                .toByteArray()
        val reopened = runBlocking { NibDocument.open(bytes) }
        val annots = reopened.annotations(0)
        assertEquals(3, annots.size)
        assertEquals(setOf("Ink", "Highlight", "Stamp"), annots.map { it.subtype }.toSet())

        // Everything was imported at open: nothing left to commit.
        val delta = reopened.exportDelta(NibDocument.ExportMode.Compacted)
        assertEquals(0, NibJson.decodeFromString<DeltaDocument>(delta).ops.size)

        // Second view on the same activity: `rule.setContent` is single-use,
        // so populate the ComposeView directly on the activity.
        lateinit var state2: NibPdfViewState
        rule.activityRule.scenario.onActivity { activity ->
            activity.setContent {
                NibPdfView(
                    rememberNibPdfViewState(reopened, FlushPolicy(enabled = false))
                        .also { s -> state2 = s },
                    Modifier.fillMaxSize(),
                )
            }
        }
        rule.waitUntil(10_000) {
            runCatching {
                state2.scheduler
                    .tilesForPage(0, state2.bucket)
                    .keys
                    .containsAll(beforeKeys)
            }.getOrDefault(false)
        }
        rule.waitForIdle()
        val after = composePageBitmap(state2, 0)
        val dist = PHash.distance(before, after)
        assertTrue("page pHash distance $dist exceeds 6", dist <= 6)
    }
}
