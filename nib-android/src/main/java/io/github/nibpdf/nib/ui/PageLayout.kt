package io.github.nibpdf.nib.ui

import io.github.nibpdf.nib.PageInfo

/**
 * Vertical page layout in **content px** (`content px = pt × density × zoom`,
 * 1 pt ≙ 1 dp at zoom 1). Pages are stacked top-to-bottom with a fixed gap
 * and horizontally centred by the caller.
 */
internal class PageLayout(
    private val pages: List<PageInfo>,
    private val density: Float,
    private val gapPx: Float,
) {
    // topOffsetsPt[i] = sum of page heights in pt above page i (no zoom, no gap).
    private val topsPt: FloatArray =
        FloatArray(pages.size + 1).also { acc ->
            pages.forEachIndexed { i, p -> acc[i + 1] = acc[i] + p.heightPt }
        }

    fun pageWidthPx(
        i: Int,
        zoom: Float,
    ): Float = pages[i].widthPt * density * zoom

    fun pageHeightPx(
        i: Int,
        zoom: Float,
    ): Float = pages[i].heightPt * density * zoom

    /** Top edge of page `i` in content px. */
    fun pageTop(
        i: Int,
        zoom: Float,
    ): Float = topsPt[i] * density * zoom + i * gapPx

    /** Total content height in px at `zoom`. */
    fun totalHeight(zoom: Float): Float = pageTop(pages.size, zoom)

    /**
     * Indices of pages intersecting `[viewportTop, viewportBottom)` in
     * content px. Linear scan is fine: it runs per scroll frame over at most
     * a few hundred pages.
     */
    fun pagesIn(
        viewportTop: Float,
        viewportBottom: Float,
        zoom: Float,
    ): IntRange {
        if (pages.isEmpty()) return IntRange.EMPTY
        var first = -1
        var last = -1
        for (i in pages.indices) {
            val top = pageTop(i, zoom)
            val bottom = top + pageHeightPx(i, zoom)
            if (bottom >= viewportTop && top < viewportBottom) {
                if (first < 0) first = i
                last = i
            } else if (first >= 0 && top >= viewportBottom) {
                break
            }
        }
        return if (first < 0) IntRange.EMPTY else first..last
    }
}
