package io.github.nibpdf.nib.delta

import android.graphics.PointF
import android.graphics.RectF
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.booleanOrNull
import kotlinx.serialization.json.contentOrNull
import kotlinx.serialization.json.floatOrNull
import kotlinx.serialization.json.intOrNull
import kotlinx.serialization.json.jsonArray
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive

/**
 * A parsed annotation entity from the delta model (JSON_SCHEMA §5).
 * Typed accessors read [json] and are `null` (or a documented default)
 * when the key is absent.
 */
public class Annotation(
    public val id: String,
    public val subtype: String,
    public val page: Int,
    /** Bounding box in user space; `null` when the entity carries none. */
    public val rect: RectF?,
    public val json: JsonObject,
) {
    private fun num(key: String): Float? = json[key]?.jsonPrimitive?.floatOrNull

    private fun str(key: String): String? = json[key]?.jsonPrimitive?.contentOrNull

    private fun bool(key: String): Boolean? = json[key]?.jsonPrimitive?.booleanOrNull

    private fun obj(key: String): JsonObject? = json[key]?.jsonObject

    private fun nestedStr(
        key: String,
        sub: String,
    ): String? = obj(key)?.get(sub)?.jsonPrimitive?.contentOrNull

    private fun nestedNum(
        key: String,
        sub: String,
    ): Float? = obj(key)?.get(sub)?.jsonPrimitive?.floatOrNull

    private fun point(key: String): PointF? =
        json[key]?.jsonArray?.let { a ->
            if (a.size >= 2) {
                PointF(a[0].jsonPrimitive.floatOrNull ?: 0f, a[1].jsonPrimitive.floatOrNull ?: 0f)
            } else {
                null
            }
        }

    public val color: String? get() = str("color")
    public val opacity: Float get() = num("opacity") ?: 1f
    public val strokeWidth: Float? get() = num("strokeWidth")

    /** Strokes as `[stroke][point][x, y, pressure?, tMs?]` (points have arity 2–4). */
    public val strokes: List<List<FloatArray>>?
        get() =
            json["strokes"]?.jsonArray?.map { stroke ->
                stroke.jsonObject["points"]?.jsonArray?.map { p ->
                    FloatArray(p.jsonArray.size) { i -> p.jsonArray[i].jsonPrimitive.floatOrNull ?: 0f }
                } ?: emptyList()
            }

    public val pressureSensitive: Boolean get() = bool("pressureSensitive") ?: false
    public val minWidthFactor: Float get() = num("minWidthFactor") ?: 0.4f
    public val maxWidthFactor: Float get() = num("maxWidthFactor") ?: 1.6f
    public val smoothingAlgorithm: String? get() =
        json["smoothing"]
            ?.jsonObject
            ?.get("algorithm")
            ?.jsonPrimitive
            ?.contentOrNull
    public val smoothingTension: Float get() =
        json["smoothing"]
            ?.jsonObject
            ?.get("tension")
            ?.jsonPrimitive
            ?.floatOrNull ?: 0.5f

    /** Markup quads, 8 numbers each. */
    public val quadPoints: List<FloatArray>?
        get() =
            json["quadPoints"]?.jsonArray?.map { q ->
                FloatArray(q.jsonArray.size) { i -> q.jsonArray[i].jsonPrimitive.floatOrNull ?: 0f }
            }

    public val blendMode: String? get() = str("blendMode")
    public val lineWidthFactor: Float get() = num("lineWidthFactor") ?: 0.06f
    public val start: PointF? get() = point("start")
    public val end: PointF? get() = point("end")
    public val lineEndings: List<String>?
        get() = json["lineEndings"]?.jsonArray?.map { it.jsonPrimitive.contentOrNull ?: "none" }
    public val contents: String? get() = str("contents")
    public val fontSize: Float get() = num("fontSize") ?: 12f
    public val fontFamily: String? get() = str("fontFamily")
    public val textAlign: String? get() = str("textAlign")
    public val fillColor: String? get() = str("fillColor")
    public val borderColor: String? get() = str("borderColor")
    public val borderWidth: Float? get() = num("borderWidth")
    public val borderStyle: String?
        get() =
            json["borderStyle"]
                ?.jsonObject
                ?.get("style")
                ?.jsonPrimitive
                ?.contentOrNull
    public val borderDash: FloatArray?
        get() =
            json["borderStyle"]?.jsonObject?.get("dash")?.jsonArray?.let { a ->
                FloatArray(a.size) { i -> a[i].jsonPrimitive.floatOrNull ?: 0f }
            }
    public val icon: String? get() = str("icon")
    public val stampKind: String? get() = str("kind")
    public val imageBase64: String?
        get() =
            json["image"]
                ?.jsonObject
                ?.get("dataBase64")
                ?.jsonPrimitive
                ?.contentOrNull
    public val imageSha256: String?
        get() =
            json["image"]
                ?.jsonObject
                ?.get("sha256")
                ?.jsonPrimitive
                ?.contentOrNull
    public val imageWidthPx: Int?
        get() =
            json["image"]
                ?.jsonObject
                ?.get("widthPx")
                ?.jsonPrimitive
                ?.intOrNull
    public val imageHeightPx: Int?
        get() =
            json["image"]
                ?.jsonObject
                ?.get("heightPx")
                ?.jsonPrimitive
                ?.intOrNull
    public val standardName: String? get() = str("standardName")
    public val vectorPaths: List<JsonObject>?
        get() =
            json["vector"]
                ?.jsonObject
                ?.get("paths")
                ?.jsonArray
                ?.map { it.jsonObject }
    public val flagsLocked: Boolean
        get() =
            json["flags"]
                ?.jsonObject
                ?.get("locked")
                ?.jsonPrimitive
                ?.booleanOrNull ?: false

    /** `flags.hidden || flags.noView` — never rendered. */
    public val flagsHidden: Boolean
        get() =
            (obj("flags")?.get("hidden")?.jsonPrimitive?.booleanOrNull ?: false) ||
                (obj("flags")?.get("noView")?.jsonPrimitive?.booleanOrNull ?: false)

    public companion object {
        public fun fromSummary(s: uniffi.nib_ffi.AnnotationSummary): Annotation {
            val obj = NibJson.parseToJsonElement(s.json).jsonObject
            val r = s.rect
            val rect =
                if (r.x1 > r.x0 && r.y1 > r.y0) {
                    RectF(r.x0.toFloat(), r.y0.toFloat(), r.x1.toFloat(), r.y1.toFloat())
                } else {
                    null
                }
            return Annotation(s.id, s.subtype, s.page.toInt(), rect, obj)
        }
    }
}
