package io.github.nibpdf.nib

import androidx.activity.ComponentActivity
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.compose.ui.test.onRoot
import androidx.compose.ui.test.performTouchInput
import androidx.compose.ui.test.swipe
import androidx.test.ext.junit.runners.AndroidJUnit4
import io.github.nibpdf.nib.ui.Tool
import kotlinx.coroutines.runBlocking
import org.junit.Assert.assertEquals
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import kotlin.concurrent.thread

/**
 * Imported annotations are already baked into the PDF: the overlay must
 * never double-draw them, even after a later flush leaves their covering
 * tiles at an older document revision.
 */
@RunWith(AndroidJUnit4::class)
class ImportedNotDoubleDrawnTest {
    @get:Rule
    val rule = createAndroidComposeRule<ComponentActivity>()

    @Test
    fun importedAnnotsStayHiddenAfterFlush() {
        val doc = runBlocking { NibDocument.open(assetBytes("annots-existing.pdf")) }
        val state = setUpView(rule, doc, Tool.Square)

        // Tiles for the imported appearances are already on screen; with
        // nothing uncommitted the overlay contributes no semantics nodes.
        rule.waitUntil(10_000) {
            state.scheduler.tilesForPage(0, state.bucket).isNotEmpty()
        }
        rule.waitForIdle()
        assertEquals(0, annotationCount(rule))

        // Create one new annotation: it is uncommitted, so only its node
        // appears until its covering tile re-renders post-flush.
        rule.onRoot().performTouchInput {
            swipe(
                start = pagePointPx(state, 0, 60f, 60f),
                end = pagePointPx(state, 0, 160f, 140f),
                durationMillis = 300,
            )
        }
        waitForAnnotationCount(rule, 1)

        thread { doc.flush() }.join()
        waitTilesAtRevision(rule, state, page = 0, revision = 1)
        rule.waitForIdle()

        // The square is now baked into a revision:1 tile and the imported
        // annotations were never overlay-drawn — zero annotation nodes.
        rule.waitUntil(10_000) { annotationCount(rule) == 0 }
    }
}
