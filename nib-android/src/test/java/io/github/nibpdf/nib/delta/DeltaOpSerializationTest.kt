package io.github.nibpdf.nib.delta

import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class DeltaOpSerializationTest {
    @Test
    fun createOpEncodesWireFormat() {
        val op =
            DeltaOps.create(
                id = "a1",
                page = 2,
                payload = JsonObject(mapOf("color" to JsonPrimitive("#FF0000"))),
            )
        val json = NibJson.parseToJsonElement(NibJson.encodeToString(DeltaOp.serializer(), op)).jsonObject
        assertEquals("create", json["type"]?.toString()?.trim('"'))
        assertEquals("annotation", json["target"]?.toString()?.trim('"'))
        assertFalse(json.containsKey("author"))
        assertFalse(json.containsKey("groupId"))
        assertTrue(json.containsKey("opId"))
        assertTrue(json.containsKey("ts"))
        assertEquals(2, json["page"].toString().toInt())
    }

    @Test
    fun decodesSchemaExampleOp() {
        val raw =
            """
            {
              "opId": "0193b2e4-7c1a-7f2e-9a53-6d2f8a1b0c44",
              "ts": "2026-11-12T14:03:25.120Z",
              "author": "Andrea",
              "groupId": "0193b2e4-7c1a-7f2e-9a53-000000000001",
              "type": "update",
              "target": "annotation",
              "id": "9b3a6c1e-1d2f-4e0a-9e4f-7a2c0b5d1f10",
              "page": 0,
              "payload": { "color": "#FF0000", "strokeWidth": 2.5 }
            }
            """.trimIndent()
        val op = NibJson.decodeFromString(DeltaOp.serializer(), raw)
        assertEquals(OpKind.Update, op.kind)
        assertEquals(OpTarget.Annotation, op.target)
        assertEquals("Andrea", op.author)
        assertEquals("0193b2e4-7c1a-7f2e-9a53-000000000001", op.groupId)
        assertEquals("#FF0000", op.payload["color"]?.jsonPrimitive?.content)
    }

    @Test
    fun uuidV7HasVersionAndVariant() {
        val id = DeltaIds.uuidV7()
        assertEquals('7', id[14])
        assertTrue(id[19] in "89ab")
    }

    @Test
    fun uuidV7IncreasesWithTime() {
        val a = DeltaIds.uuidV7()
        Thread.sleep(2)
        val b = DeltaIds.uuidV7()
        assertTrue(a < b)
    }

    @Test
    fun rfc3339NowFormat() {
        assertTrue(Rfc3339.now().matches(Regex("""^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}\.\d{3}Z$""")))
    }

    @Test
    fun updateOmitsPageWhenNull() {
        val op = DeltaOps.update(id = "x", patch = JsonObject(emptyMap()))
        assertNull(op.page)
        val json = NibJson.parseToJsonElement(NibJson.encodeToString(DeltaOp.serializer(), op)).jsonObject
        assertFalse(json.containsKey("page"))
        assertEquals("update", json["type"]?.toString()?.trim('"'))
    }
}
