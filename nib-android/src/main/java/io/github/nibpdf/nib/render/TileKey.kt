package io.github.nibpdf.nib.render

/** Identity of one rendered tile at one zoom bucket. */
public data class TileKey(
    val page: Int,
    val bucket: Int,
    val tx: Int,
    val ty: Int,
)
