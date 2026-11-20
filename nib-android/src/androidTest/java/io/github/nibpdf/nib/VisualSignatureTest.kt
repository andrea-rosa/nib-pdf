package io.github.nibpdf.nib

import android.graphics.Bitmap
import android.util.Base64
import androidx.activity.ComponentActivity
import androidx.compose.ui.graphics.asAndroidBitmap
import androidx.compose.ui.test.assertIsNotEnabled
import androidx.compose.ui.test.captureToImage
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onRoot
import androidx.compose.ui.test.performClick
import androidx.test.ext.junit.runners.AndroidJUnit4
import io.github.nibpdf.nib.delta.DeltaIds
import io.github.nibpdf.nib.delta.DeltaOps
import io.github.nibpdf.nib.delta.SignatureFieldState
import io.github.nibpdf.nib.ui.Tool
import io.github.nibpdf.nib.ui.VisualSignatureRequest
import io.github.nibpdf.nib.ui.toComposeRect
import kotlinx.coroutines.runBlocking
import kotlinx.serialization.json.add
import kotlinx.serialization.json.buildJsonArray
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.contentOrNull
import kotlinx.serialization.json.jsonPrimitive
import kotlinx.serialization.json.put
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import java.io.ByteArrayOutputStream
import kotlin.concurrent.thread
import kotlin.math.abs

@RunWith(AndroidJUnit4::class)
class VisualSignatureTest {
    @get:Rule
    val rule = createAndroidComposeRule<ComponentActivity>()

    @Test
    fun tappingEmptyWidgetRequestsMarkAndBindsIt() {
        val doc = runBlocking { NibDocument.open(assetBytes("sig-field.pdf")) }
        val state = setUpView(rule, doc, Tool.Form)
        rule.onNodeWithTag("field:Signature1", useUnmergedTree = true).assertIsNotEnabled()
        var request: VisualSignatureRequest? = null
        lateinit var annotationId: String
        rule.runOnIdle { state.onRequestVisualSignature = { request = it } }
        rule.onNodeWithTag("field:Signature1", useUnmergedTree = true).performClick()
        rule.runOnIdle {
            val selected = requireNotNull(request)
            val field = requireNotNull(doc.formField("Signature1"))
            assertEquals(0, selected.page)
            assertEquals(field.widgets.single().id, selected.widgetId)
            assertEquals(field.widgets.single().rect, selected.rect)
            val payload =
                buildJsonObject {
                    put("subtype", "Ink")
                    put("page", selected.page)
                    put("intent", "signature")
                    put("bindToField", selected.fieldName)
                    put("bindToWidget", requireNotNull(selected.widgetId))
                    put("fitMode", "contain")
                    put("strokeWidth", 2)
                    put(
                        "strokes",
                        buildJsonArray {
                            add(
                                buildJsonObject {
                                    put(
                                        "points",
                                        buildJsonArray {
                                            add(
                                                buildJsonArray {
                                                    add(0)
                                                    add(0)
                                                },
                                            )
                                            add(
                                                buildJsonArray {
                                                    add(100)
                                                    add(20)
                                                },
                                            )
                                        },
                                    )
                                },
                            )
                        },
                    )
                }
            annotationId = DeltaIds.uuidV4()
            doc.apply(DeltaOps.create(annotationId, selected.page, payload))
            val annotation = requireNotNull(doc.annotation(annotationId))
            assertEquals(selected.rect, annotation.rect)
            assertTrue(annotation.flagsLocked)
            assertEquals(SignatureFieldState.Empty, doc.formField("Signature1")?.signatureState)
            assertNotNull(doc.undo())
            assertNotNull(doc.redo())
            assertEquals(selected.rect, doc.annotation(annotationId)?.rect)
        }

        fun bluePixels(): Int {
            val bitmap = rule.onRoot().captureToImage().asAndroidBitmap()
            val topLeft = pagePointPx(state, 0, 40f, 150f)
            val bottomRight = pagePointPx(state, 0, 260f, 70f)
            var count = 0
            for (y in topLeft.y.toInt().coerceAtLeast(0) until bottomRight.y.toInt().coerceAtMost(bitmap.height)) {
                for (x in topLeft.x.toInt().coerceAtLeast(0) until bottomRight.x.toInt().coerceAtMost(bitmap.width)) {
                    val pixel = bitmap.getPixel(x, y)
                    val red = android.graphics.Color.red(pixel)
                    val green = android.graphics.Color.green(pixel)
                    val blue = android.graphics.Color.blue(pixel)
                    if (blue > red + 15 && blue > green + 15 && blue < 210) count++
                }
            }
            return count
        }
        assertTrue("unflushed visual mark is hidden", bluePixels() > 5)
        var revision = -1
        thread { revision = doc.flush().revision }.join()
        waitTilesAtRevision(rule, state, 0, revision)
        assertTrue("flushed visual mark is hidden", bluePixels() > 5)
        val reopened = runBlocking { NibDocument.open(doc.bytes()) }
        assertEquals(request?.rect, reopened.annotation(annotationId)?.rect)
        assertTrue(requireNotNull(reopened.annotation(annotationId)).flagsLocked)
        assertEquals(SignatureFieldState.Empty, reopened.formField("Signature1")?.signatureState)
        reopened.close()
        doc.close()
    }

    @Test
    fun occupiedWidgetIsDisabledEvenWithAHostHook() {
        val doc = runBlocking { NibDocument.open(assetBytes("incr-presigned.pdf")) }
        val state = setUpView(rule, doc, Tool.Form)
        rule.runOnIdle { state.onRequestVisualSignature = {} }
        assertEquals(SignatureFieldState.Occupied, doc.formField("Signature1")?.signatureState)
        rule.onNodeWithTag("field:Signature1", useUnmergedTree = true).assertIsNotEnabled()
        doc.close()
    }

    @Test
    fun rotatedBoundImageMatchesFlushedAppearance() {
        val bitmap = Bitmap.createBitmap(80, 40, Bitmap.Config.ARGB_8888)
        bitmap.eraseColor(android.graphics.Color.TRANSPARENT)
        val canvas = android.graphics.Canvas(bitmap)
        val paint = android.graphics.Paint().apply { color = android.graphics.Color.BLUE }
        canvas.drawRect(0f, 0f, 55f, 15f, paint)
        paint.color = android.graphics.Color.RED
        canvas.drawRect(50f, 15f, 80f, 40f, paint)
        val bytes = ByteArrayOutputStream()
        assertTrue(bitmap.compress(Bitmap.CompressFormat.PNG, 100, bytes))
        val image =
            buildJsonObject {
                put("mime", "image/png")
                put("dataBase64", Base64.encodeToString(bytes.toByteArray(), Base64.NO_WRAP))
            }
        assertRotatedStampParity(
            buildJsonObject {
                put("kind", "image")
                put("image", image)
            },
        )
    }

    @Test
    fun rotatedBoundVectorMatchesFlushedAppearance() {
        assertRotatedStampParity(
            buildJsonObject {
                put("kind", "vector")
                put(
                    "vector",
                    buildJsonObject {
                        put(
                            "viewBox",
                            buildJsonArray {
                                add(0)
                                add(0)
                                add(100)
                                add(50)
                            },
                        )
                        put(
                            "paths",
                            buildJsonArray {
                                add(
                                    buildJsonObject {
                                        put("d", "M 0 0 L 100 0 L 0 50 Z")
                                        put("fill", "#0000FF")
                                    },
                                )
                            },
                        )
                    },
                )
            },
        )
    }

    @Test
    fun rotatedBoundStandardMatchesFlushedAppearance() {
        assertRotatedStampParity(buildJsonObject { put("kind", "standard") })
    }

    private fun assertRotatedStampParity(graphic: kotlinx.serialization.json.JsonObject) {
        val doc = runBlocking { NibDocument.open(assetBytes("sig-field-rotated.pdf")) }
        val state = setUpView(rule, doc, Tool.Form)
        val field = requireNotNull(doc.formField("Signature1"))
        val widget = field.widgets.single()

        fun screenshot(): Bitmap {
            val bitmap = rule.onRoot().captureToImage().asAndroidBitmap()
            val rect = state.transform(0).rectToContent(widget.rect.toComposeRect())
            val origin = state.pageOriginInView(0)
            val x = (origin.x + rect.left).toInt().coerceIn(0, bitmap.width - 1)
            val y = (origin.y + rect.top).toInt().coerceIn(0, bitmap.height - 1)
            val width =
                rect.width
                    .toInt()
                    .coerceAtLeast(1)
                    .coerceAtMost(bitmap.width - x)
            val height =
                rect.height
                    .toInt()
                    .coerceAtLeast(1)
                    .coerceAtMost(bitmap.height - y)
            return Bitmap.createBitmap(bitmap, x, y, width, height)
        }
        val blank = screenshot()
        val payload =
            buildJsonObject {
                graphic.forEach { (key, value) -> put(key, value) }
                put("subtype", "Stamp")
                put("page", 0)
                put("intent", "signature")
                put("bindToField", field.name)
                put("bindToWidget", requireNotNull(widget.id))
                put("fitMode", "contain")
            }
        rule.runOnIdle { doc.apply(DeltaOps.create(DeltaIds.uuidV4(), 0, payload)) }
        rule.waitForIdle()
        val live = screenshot()
        var revision = -1
        thread { revision = doc.flush().revision }.join()
        waitTilesAtRevision(rule, state, 0, revision)
        rule.waitForIdle()
        val flushed = screenshot()
        val distance = PHash.distance(live, flushed)
        if (graphic["kind"]?.jsonPrimitive?.contentOrNull == "standard") {
            val livePixels = IntArray(live.width * live.height)
            val flushedPixels = IntArray(flushed.width * flushed.height)
            live.getPixels(livePixels, 0, live.width, 0, 0, live.width, live.height)
            flushed.getPixels(flushedPixels, 0, flushed.width, 0, 0, flushed.width, flushed.height)
            val different =
                livePixels.indices.count { index ->
                    val a = livePixels[index]
                    val b = flushedPixels[index]
                    maxOf(
                        abs(android.graphics.Color.red(a) - android.graphics.Color.red(b)),
                        abs(android.graphics.Color.green(a) - android.graphics.Color.green(b)),
                        abs(android.graphics.Color.blue(a) - android.graphics.Color.blue(b)),
                    ) > 32
                }
            val fraction = different.toFloat() / livePixels.size
            assertTrue("live vs flushed standard stamp differs in $fraction of pixels", fraction <= 0.08f)
        } else {
            assertTrue("live vs flushed signature pHash=$distance", distance <= 6)
        }
        assertTrue("signature was not rendered", PHash.distance(blank, flushed) > 6)
        doc.close()
    }
}
