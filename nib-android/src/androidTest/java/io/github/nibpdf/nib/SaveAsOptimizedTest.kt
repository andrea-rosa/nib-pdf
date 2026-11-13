package io.github.nibpdf.nib

import androidx.test.ext.junit.runners.AndroidJUnit4
import io.github.nibpdf.nib.delta.DeltaIds
import io.github.nibpdf.nib.delta.DeltaOps
import io.github.nibpdf.nib.delta.Rfc3339
import kotlinx.coroutines.runBlocking
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.buildJsonArray
import kotlinx.serialization.json.buildJsonObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import java.io.ByteArrayOutputStream

/**
 * `save(optimized = true)` performs a compacting full rewrite; the result
 * re-opens with the same annotations and is no larger than the incremental.
 */
@RunWith(AndroidJUnit4::class)
class SaveAsOptimizedTest {
    @Test
    fun optimizedRewritesAndPreserves() {
        val doc = runBlocking { NibDocument.open(assetBytes("text.pdf")) }
        repeat(3) { i ->
            doc.apply(
                DeltaOps.create(
                    DeltaIds.uuidV4(),
                    0,
                    buildJsonObject {
                        put("subtype", JsonPrimitive("Square"))
                        put("page", JsonPrimitive(0))
                        put(
                            "rect",
                            buildJsonArray {
                                listOf(100.0 + i * 10, 100.0 + i * 10, 200.0 + i * 10, 200.0 + i * 10)
                                    .forEach { add(JsonPrimitive(it)) }
                            },
                        )
                        put("color", JsonPrimitive("#1A237E"))
                        put("strokeWidth", JsonPrimitive(2.0))
                        put("createdAt", JsonPrimitive(Rfc3339.now()))
                    },
                ),
            )
        }

        val bytesA =
            ByteArrayOutputStream()
                .also { out -> runBlocking { doc.save(out, optimized = false) } }
                .toByteArray()
        val bytesB =
            ByteArrayOutputStream()
                .also { out -> runBlocking { doc.save(out, optimized = true) } }
                .toByteArray()

        assertEquals(3, runBlocking { NibDocument.open(bytesA) }.annotations(0).size)
        assertEquals(3, runBlocking { NibDocument.open(bytesB) }.annotations(0).size)
        assertTrue(
            "optimized (${bytesB.size}) larger than incremental (${bytesA.size})",
            bytesB.size <= bytesA.size,
        )
    }
}
