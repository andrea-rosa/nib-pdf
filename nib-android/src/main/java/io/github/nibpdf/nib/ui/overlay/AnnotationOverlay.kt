package io.github.nibpdf.nib.ui.overlay

import androidx.compose.foundation.Canvas
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.size
import androidx.compose.runtime.Composable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.rememberTextMeasurer
import androidx.compose.ui.unit.IntOffset
import io.github.nibpdf.nib.ui.NibPdfViewState
import io.github.nibpdf.nib.ui.toComposeRect
import kotlin.math.roundToInt

/**
 * Draws every committed annotation on [page] in z-order, plus one
 * non-interactive semantics box per annotation for tests.
 */
@Composable
@Suppress("ktlint:standard:function-naming")
internal fun AnnotationOverlay(
    state: NibPdfViewState,
    page: Int,
    modifier: Modifier = Modifier,
) {
    val version by state.document.annotationsVersion.collectAsState()
    val annotations = remember(version, page) { state.document.annotations(page) }
    val t = state.transform(page)
    val k = t.k
    val textMeasurer = rememberTextMeasurer()
    val images = remember { StampImageCache() }
    Canvas(modifier.fillMaxSize()) {
        annotations.forEach { a ->
            AnnotationDrawing.drawAnnotation(this, a, t, k, textMeasurer, images)
        }
    }
    annotations.forEach { a ->
        val r = a.rect?.let { t.rectToContent(it.toComposeRect()) }
        with(LocalDensity.current) {
            Box(
                Modifier
                    .offset {
                        if (r != null) {
                            IntOffset(r.left.roundToInt(), r.top.roundToInt())
                        } else {
                            IntOffset.Zero
                        }
                    }.size(
                        (r?.width ?: 1f).toDp(),
                        (r?.height ?: 1f).toDp(),
                    ).testTag("annot:${a.id}")
                    .semantics { contentDescription = "annotation:${a.subtype}" },
            )
        }
    }
}
