package io.github.nibpdf.nib

import androidx.test.ext.junit.runners.AndroidJUnit4
import io.github.nibpdf.nib.delta.DeltaIds
import io.github.nibpdf.nib.delta.DeltaOps
import io.github.nibpdf.nib.delta.Rfc3339
import kotlinx.coroutines.runBlocking
import kotlinx.serialization.json.add
import kotlinx.serialization.json.buildJsonArray
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.put
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith

@RunWith(AndroidJUnit4::class)
class FreeTextFontProviderTest {
    @Test
    fun publicFontProviderEmbedsAndReopensNonWinAnsiText() {
        val doc = runBlocking { NibDocument.open(assetBytes("text.pdf")) }
        val font = assetBytes("fonts/noto-sans-sc-subset.ttf")
        var providerCalls = 0
        doc.setFontProvider { family, bold, italic ->
            assertEquals("Noto Sans SC", family)
            assertTrue(!bold)
            assertTrue(!italic)
            providerCalls++
            font
        }

        val contents = "Latin café — Привет мир — 中文"
        val annotationId = DeltaIds.uuidV4()
        doc.apply(
            DeltaOps.create(
                annotationId,
                0,
                buildJsonObject {
                    put("subtype", "FreeText")
                    put("page", 0)
                    put(
                        "rect",
                        buildJsonArray {
                            listOf(40, 40, 360, 140).forEach { add(it) }
                        },
                    )
                    put("contents", contents)
                    put("fontFamily", "Noto Sans SC")
                    put("fontSize", 18)
                    put("createdAt", Rfc3339.now())
                },
            ),
        )

        val report = doc.flush()
        assertTrue("unexpected font warnings: ${report.warnings}", report.warnings.isEmpty())
        assertEquals(1, providerCalls)
        val bytes = doc.bytes()
        val serializedPdf = bytes.toString(Charsets.ISO_8859_1)
        assertTrue("embedded Type0 font resource missing", serializedPdf.contains("/NIBF"))
        assertTrue("embedded font program missing", serializedPdf.contains("/FontFile2"))
        assertTrue("Unicode CMap missing", serializedPdf.contains("/ToUnicode"))
        val reopened = runBlocking { NibDocument.open(bytes) }
        assertEquals(contents, reopened.annotation(annotationId)?.contents)
        reopened.close()
        doc.close()
    }
}
