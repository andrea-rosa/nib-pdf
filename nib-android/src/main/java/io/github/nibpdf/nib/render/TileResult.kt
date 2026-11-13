package io.github.nibpdf.nib.render

import android.graphics.Bitmap
import java.nio.ByteBuffer

/**
 * Converts a rendered RGBA tile into a pooled `ARGB_8888` bitmap.
 * (`uniffi.nib_ffi.TileResult.pixels` is RGBA, `stride = width * 4`.)
 */
internal fun uniffi.nib_ffi.TileResult.toBitmap(pool: BitmapPool): Bitmap {
    val bmp = pool.obtain(width.toInt(), height.toInt())
    bmp.copyPixelsFromBuffer(ByteBuffer.wrap(pixels))
    return bmp
}
