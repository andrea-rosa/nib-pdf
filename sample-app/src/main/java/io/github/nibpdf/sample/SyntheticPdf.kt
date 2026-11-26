package io.github.nibpdf.sample

import java.io.ByteArrayOutputStream

/**
 * Deterministic uncompressed text PDFs for the `:macrobenchmark` budgets
 * (ROADMAP §7: 10 MB first-visible-page, 20 MB flush). Generated at launch
 * so multi-megabyte inputs never enter the repository. Letter pages, 100
 * lines of 7 pt Helvetica each (~15 KB per page), until [text] reaches the
 * requested size.
 */
internal object SyntheticPdf {
    private val WORDS =
        (
            "lorem ipsum dolor sit amet consectetur adipiscing elit sed do eiusmod tempor " +
                "incididunt ut labore et dolore magna aliqua enim ad minim veniam quis nostrud " +
                "exercitation ullamco laboris nisi aliquip ex ea commodo consequat duis aute irure"
        ).split(' ')

    private const val LINES_PER_PAGE = 100
    private const val CHARS_PER_LINE = 140

    /** Parses `text:<megabytes>` (the `nib.synthetic` launch extra). */
    fun fromSpec(spec: String): ByteArray {
        val mb =
            spec.removePrefix("text:").toIntOrNull()?.takeIf { spec.startsWith("text:") && it in 1..64 }
                ?: throw IllegalArgumentException("unsupported nib.synthetic spec: $spec")
        return text(mb * 1024 * 1024)
    }

    fun text(targetBytes: Int): ByteArray {
        val out = ByteArrayOutputStream(targetBytes + 64 * 1024)
        val offsets = HashMap<Int, Int>()

        fun write(s: String) = out.write(s.toByteArray(Charsets.ISO_8859_1))

        fun obj(
            id: Int,
            body: String,
        ) {
            offsets[id] = out.size()
            write("$id 0 obj\n$body\nendobj\n")
        }

        write("%PDF-1.4\n%\u00e2\u00e3\u00cf\u00d3\n")
        obj(1, "<< /Type /Catalog /Pages 2 0 R >>")
        obj(3, "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>")
        var pages = 0
        while (out.size() < targetBytes) {
            val content = pageContent(pages)
            val pageId = 4 + 2 * pages
            obj(
                pageId,
                "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] " +
                    "/Resources << /Font << /F1 3 0 R >> >> /Contents ${pageId + 1} 0 R >>",
            )
            obj(pageId + 1, "<< /Length ${content.length} >>\nstream\n$content\nendstream")
            pages++
        }
        val kids = (0 until pages).joinToString(" ") { "${4 + 2 * it} 0 R" }
        obj(2, "<< /Type /Pages /Kids [$kids] /Count $pages >>")

        val size = 4 + 2 * pages
        val xref = out.size()
        val table = StringBuilder("xref\n0 $size\n0000000000 65535 f \n")
        for (id in 1 until size) {
            table.append(String.format(java.util.Locale.ROOT, "%010d 00000 n \n", offsets.getValue(id)))
        }
        write(table.toString())
        write("trailer\n<< /Size $size /Root 1 0 R >>\nstartxref\n$xref\n%%EOF\n")
        return out.toByteArray()
    }

    private fun pageContent(page: Int): String {
        val sb = StringBuilder(LINES_PER_PAGE * (CHARS_PER_LINE + 8) + 64)
        sb.append("BT\n/F1 7 Tf\n7.6 TL\n36 772 Td\n")
        // LCG keeps the text varied but byte-identical across runs.
        var seed = page.toLong() * 7919L + 17L
        for (line in 0 until LINES_PER_PAGE) {
            val text = StringBuilder("p${page + 1} l${line + 1}")
            while (text.length < CHARS_PER_LINE) {
                seed = (seed * 6364136223846793005L + 1442695040888963407L)
                text.append(' ').append(WORDS[((seed ushr 33) % WORDS.size).toInt()])
            }
            sb.append('(').append(text, 0, CHARS_PER_LINE).append(") Tj T*\n")
        }
        sb.append("ET")
        return sb.toString()
    }
}
