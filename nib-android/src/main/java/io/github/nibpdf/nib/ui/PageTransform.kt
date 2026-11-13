package io.github.nibpdf.nib.ui

import android.graphics.RectF
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.geometry.Size
import io.github.nibpdf.nib.PageRotation

/**
 * User-space ↔ content-space transform for one page.
 *
 * User space is PDF points, y-up, inside the page's `/CropBox`. Content space
 * is device px, y-down, origin at the page's top-left corner (`k` px per pt).
 * Port of `nib-core/src/geom/transform.rs`; rotation table:
 *
 * | `/Rotate` | user→content for `(x,y)`        |
 * |-----------|---------------------------------|
 * | 0         | `(k(x−x0), k(y1−y))`            |
 * | 90        | `(k(y−y0), k(x−x0))`            |
 * | 180       | `(k(x1−x), k(y−y0))`            |
 * | 270       | `(k(y1−y), k(x1−x))`            |
 */
internal class PageTransform(
    cropBox: Rect,
    private val rotation: PageRotation,
    private val k: Float,
) {
    private val x0 = cropBox.left
    private val y0 = cropBox.top
    private val x1 = cropBox.right
    private val y1 = cropBox.bottom

    /** Content-space page size in px; swapped for 90/270. */
    val contentSize: Size =
        if (rotation == PageRotation.R90 || rotation == PageRotation.R270) {
            Size((y1 - y0) * k, (x1 - x0) * k)
        } else {
            Size((x1 - x0) * k, (y1 - y0) * k)
        }

    fun toContent(user: Offset): Offset =
        when (rotation) {
            PageRotation.R0 -> Offset(k * (user.x - x0), k * (y1 - user.y))
            PageRotation.R90 -> Offset(k * (user.y - y0), k * (user.x - x0))
            PageRotation.R180 -> Offset(k * (x1 - user.x), k * (user.y - y0))
            PageRotation.R270 -> Offset(k * (y1 - user.y), k * (x1 - user.x))
        }

    fun toUser(content: Offset): Offset =
        when (rotation) {
            PageRotation.R0 -> Offset(content.x / k + x0, y1 - content.y / k)
            PageRotation.R90 -> Offset(content.y / k + x0, content.x / k + y0)
            PageRotation.R180 -> Offset(x1 - content.x / k, content.y / k + y0)
            PageRotation.R270 -> Offset(x1 - content.y / k, y1 - content.x / k)
        }

    private fun mapRect(
        r: Rect,
        map: (Offset) -> Offset,
    ): Rect {
        val corners =
            arrayOf(
                map(Offset(r.left, r.top)),
                map(Offset(r.right, r.top)),
                map(Offset(r.right, r.bottom)),
                map(Offset(r.left, r.bottom)),
            )
        return Rect(
            left = corners.minOf { it.x },
            top = corners.minOf { it.y },
            right = corners.maxOf { it.x },
            bottom = corners.maxOf { it.y },
        )
    }

    fun rectToContent(r: Rect): Rect = mapRect(r, ::toContent)

    fun rectToUser(r: Rect): Rect = mapRect(r, ::toUser)
}

/** User-space `RectF` (left,top,right,bottom = x0,y0,x1,y1) → Compose [Rect]. */
internal fun RectF.toComposeRect(): Rect = Rect(left, top, right, bottom)
