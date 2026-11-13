package io.github.nibpdf.nib

import androidx.activity.ComponentActivity
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.compose.ui.test.onAllNodesWithTag
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onRoot
import androidx.compose.ui.test.performTouchInput
import androidx.compose.ui.test.swipe
import androidx.test.ext.junit.runners.AndroidJUnit4
import io.github.nibpdf.nib.ui.Tool
import kotlinx.coroutines.runBlocking
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import kotlin.concurrent.thread

/**
 * Overlay-until-flushed: after `flush()` the overlay keeps drawing a
 * committed annotation until every covering tile re-renders at the new
 * revision — no blank flicker between flush and tile swap.
 */
@RunWith(AndroidJUnit4::class)
class NoFlickerTest {
    @get:Rule
    val rule = createAndroidComposeRule<ComponentActivity>()

    @Test
    fun overlayUntilTilesCatchUp() {
        val doc = runBlocking { NibDocument.open(assetBytes("text.pdf")) }
        val state = setUpView(rule, doc, Tool.Square)

        rule.onRoot().performTouchInput {
            swipe(
                start = pagePointPx(state, 0, 60f, 60f),
                end = pagePointPx(state, 0, 160f, 140f),
                durationMillis = 300,
            )
        }
        waitForAnnotationCount(rule, 1)
        val id = doc.annotations(0).single { it.subtype == "Square" }.id

        // Freeze the scheduler: flush commits the annotation but no tile
        // re-render is allowed to start.
        rule.runOnIdle { state.scheduler.paused = true }
        var revision = -1
        thread { revision = doc.flush().revision }.join()
        assertEquals(1, revision)
        assertEquals(1, doc.revision.value)

        // Committed but not yet on screen in tiles: the overlay keeps the
        // semantics node and drawing alive.
        rule.waitForIdle()
        rule.onNodeWithTag("annot:$id", useUnmergedTree = true).assertExists()
        assertTrue(
            "covering tile unexpectedly re-rendered while paused",
            tilesCoveringDirty(state, 0).all {
                (state.scheduler.revisionOf(it) ?: 0) < 1
            },
        )

        // Unfreeze: tiles covering the dirty rect re-render at revision 1.
        rule.runOnIdle {
            state.scheduler.paused = false
            state.updateScheduler()
        }
        waitTilesAtRevision(rule, state, page = 0, revision = 1)
        rule.waitForIdle()

        // The covering tile's semantics node reports the new revision…
        val covering = tilesCoveringDirty(state, 0).first()
        val node =
            rule
                .onNodeWithTag(
                    "tile:${covering.page}:${covering.tx}:${covering.ty}",
                    useUnmergedTree = true,
                ).fetchSemanticsNode()
        assertEquals("revision:1", node.config[SemanticsProperties.StateDescription])

        // …and the overlay drops the annotation once every covering tile
        // shows the flushed appearance.
        rule.waitUntil(10_000) {
            rule
                .onAllNodesWithTag("annot:$id", useUnmergedTree = true)
                .fetchSemanticsNodes()
                .isEmpty()
        }
    }
}
