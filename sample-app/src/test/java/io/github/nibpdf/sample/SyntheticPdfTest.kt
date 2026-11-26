package io.github.nibpdf.sample

import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import java.io.File

class SyntheticPdfTest {
    @Test
    fun sizeIsCloseToTarget() {
        val mb = 1024 * 1024
        val pdf = SyntheticPdf.fromSpec("text:2")
        assertTrue("size ${pdf.size}", pdf.size in 2 * mb until 2 * mb + 64 * 1024)
    }

    @Test
    fun isDeterministic() {
        assertArrayEquals(SyntheticPdf.text(200_000), SyntheticPdf.text(200_000))
    }

    @Test
    fun xrefOffsetsPointAtObjects() {
        val pdf = SyntheticPdf.text(300_000)
        val text = String(pdf, Charsets.ISO_8859_1)
        val startxref =
            text
                .substringAfterLast("startxref\n")
                .substringBefore('\n')
                .trim()
                .toInt()
        assertTrue(text.startsWith("xref", startxref))
        val header = text.substring(startxref).lines()[1].split(' ')
        val count = header[1].toInt()
        for (n in 1 until count) {
            val line = text.substring(startxref).lines()[2 + n]
            val offset = line.substring(0, 10).toInt()
            assertTrue("obj $n at $offset", text.startsWith("$n 0 obj", offset))
        }
        assertTrue(text.trimEnd().endsWith("%%EOF"))
        System.getenv("NIB_SYNTHETIC_OUT")?.let { File(it).writeBytes(pdf) }
    }

    @Test(expected = IllegalArgumentException::class)
    fun rejectsUnknownSpec() {
        SyntheticPdf.fromSpec("image:3")
    }

    @Test
    fun parsesSize() {
        assertEquals(SyntheticPdf.fromSpec("text:1").size, SyntheticPdf.text(1024 * 1024).size)
    }
}
