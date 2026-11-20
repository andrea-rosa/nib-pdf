package io.github.nibpdf.nib.delta

import android.graphics.RectF
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.booleanOrNull
import kotlinx.serialization.json.contentOrNull
import kotlinx.serialization.json.floatOrNull
import kotlinx.serialization.json.intOrNull
import kotlinx.serialization.json.jsonArray
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive

/** AcroForm field type (JSON_SCHEMA §7.1 `fieldType`). */
public enum class FieldType {
    Text,
    Checkbox,
    Radio,
    PushButton,
    Combo,
    List,
    Signature,
    ;

    public companion object {
        public fun parse(s: String): FieldType =
            when (s) {
                "text" -> Text
                "checkbox" -> Checkbox
                "radio" -> Radio
                "pushButton" -> PushButton
                "combo" -> Combo
                "list" -> List
                "signature" -> Signature
                else -> Text
            }
    }
}

/** Occupancy of a signature field; this does not establish signature validity. */
public enum class SignatureFieldState {
    Empty,
    Occupied,
    Unknown,
}

/** `/Ff` bits, decoded (JSON_SCHEMA §7.2). */
public data class FieldFlags(
    val readOnly: Boolean = false,
    val required: Boolean = false,
    val noExport: Boolean = false,
    val multiline: Boolean = false,
    val password: Boolean = false,
    val comb: Boolean = false,
    val doNotSpellCheck: Boolean = false,
    val doNotScroll: Boolean = false,
    val richText: Boolean = false,
    val noToggleToOff: Boolean = false,
    val radiosInUnison: Boolean = false,
    val editable: Boolean = false,
    val sort: Boolean = false,
    val multiSelect: Boolean = false,
) {
    public companion object {
        public fun fromJson(o: JsonObject?): FieldFlags =
            if (o == null) {
                FieldFlags()
            } else {
                fun b(k: String) = o[k]?.jsonPrimitive?.booleanOrNull ?: false
                FieldFlags(
                    readOnly = b("readOnly"),
                    required = b("required"),
                    noExport = b("noExport"),
                    multiline = b("multiline"),
                    password = b("password"),
                    comb = b("comb"),
                    doNotSpellCheck = b("doNotSpellCheck"),
                    doNotScroll = b("doNotScroll"),
                    richText = b("richText"),
                    noToggleToOff = b("noToggleToOff"),
                    radiosInUnison = b("radiosInUnison"),
                    editable = b("editable"),
                    sort = b("sort"),
                    multiSelect = b("multiSelect"),
                )
            }
    }
}

/** One `/Opt` entry: [export] is the stored value, [label] what readers show. */
public data class FieldOption(
    val export: String,
    val label: String? = null,
) {
    /** Label when present, else the export value. */
    public val display: String get() = label ?: export
}

/** Widget annotation of a field (JSON_SCHEMA §7.1 `widgets[]`). */
public data class Widget(
    val id: String?,
    val page: Int,
    val rect: RectF,
    val appearanceState: String?,
    val exportValue: String?,
    val borderColor: String?,
    val backgroundColor: String?,
    val borderWidth: Float?,
) {
    public companion object {
        public fun fromJson(o: JsonObject): Widget {
            val r = o["rect"]?.jsonArray
            return Widget(
                id = o["id"]?.jsonPrimitive?.contentOrNull,
                page = o["page"]?.jsonPrimitive?.intOrNull ?: 0,
                rect =
                    r?.let { a ->
                        RectF(
                            a[0].jsonPrimitive.floatOrNull ?: 0f,
                            a[1].jsonPrimitive.floatOrNull ?: 0f,
                            a[2].jsonPrimitive.floatOrNull ?: 0f,
                            a[3].jsonPrimitive.floatOrNull ?: 0f,
                        )
                    } ?: RectF(),
                appearanceState = o["appearanceState"]?.jsonPrimitive?.contentOrNull,
                exportValue = o["exportValue"]?.jsonPrimitive?.contentOrNull,
                borderColor = o["borderColor"]?.jsonPrimitive?.contentOrNull,
                backgroundColor = o["backgroundColor"]?.jsonPrimitive?.contentOrNull,
                borderWidth = o["borderWidth"]?.jsonPrimitive?.floatOrNull,
            )
        }
    }
}

/**
 * An AcroForm field entity (JSON_SCHEMA §7). Typed accessors parse [json]
 * once; absent keys become `null`/empty as documented.
 */
public data class FormField(
    val name: String,
    val fieldType: FieldType,
    /** Raw `value` member; `null` when unset (may be JsonArray for lists). */
    val value: JsonElement?,
    val maxLen: Int?,
    val flags: FieldFlags,
    /** `textAlign`/`defaultAppearance`/`fontSize`/`textColor` for editors. */
    val textAlign: String?,
    val fontSize: Float?,
    val textColor: String?,
    val options: List<FieldOption>,
    val exportValues: List<String>,
    val topIndex: Int?,
    val widgets: List<Widget>,
    val json: JsonObject,
    val signatureState: SignatureFieldState? = null,
) {
    /** `value` as a plain string, or `null` (arrays → `null`, see [valueList]). */
    public val valueString: String?
        get() =
            when (val v = value) {
                null -> null
                is JsonArray -> null
                else -> v.jsonPrimitive.contentOrNull
            }

    /** `value` as strings — string → singleton, array → elements. */
    public val valueList: List<String>
        get() =
            when (val v = value) {
                null -> emptyList()
                is JsonArray -> v.mapNotNull { it.jsonPrimitive.contentOrNull }
                else -> listOfNotNull(v.jsonPrimitive.contentOrNull)
            }

    /** First widget on [page], in widget order. */
    public fun widgetOn(page: Int): Widget? = widgets.firstOrNull { it.page == page }

    public companion object {
        public fun fromSummary(s: uniffi.nib_ffi.FormFieldSummary): FormField = fromJson(NibJson.parseToJsonElement(s.json).jsonObject)

        public fun fromJson(obj: JsonObject): FormField {
            val da = obj["defaultAppearance"]?.jsonObject
            val fieldType = FieldType.parse(obj["fieldType"]?.jsonPrimitive?.contentOrNull ?: "text")
            return FormField(
                name = obj["name"]?.jsonPrimitive?.contentOrNull ?: "",
                fieldType = fieldType,
                value = obj["value"],
                maxLen = obj["maxLen"]?.jsonPrimitive?.intOrNull,
                flags = FieldFlags.fromJson(obj["flags"]?.jsonObject),
                textAlign = obj["textAlign"]?.jsonPrimitive?.contentOrNull,
                fontSize = da?.get("fontSize")?.jsonPrimitive?.floatOrNull,
                textColor = da?.get("color")?.jsonPrimitive?.contentOrNull,
                options =
                    obj["options"]?.jsonArray?.map { o ->
                        FieldOption(
                            export = o.jsonObject["export"]?.jsonPrimitive?.contentOrNull ?: "",
                            label = o.jsonObject["label"]?.jsonPrimitive?.contentOrNull,
                        )
                    } ?: emptyList(),
                exportValues =
                    obj["exportValues"]?.jsonArray?.mapNotNull {
                        it.jsonPrimitive.contentOrNull
                    } ?: emptyList(),
                topIndex = obj["topIndex"]?.jsonPrimitive?.intOrNull,
                widgets =
                    obj["widgets"]?.jsonArray?.map {
                        Widget.fromJson(it.jsonObject)
                    } ?: emptyList(),
                json = obj,
                signatureState =
                    if (fieldType == FieldType.Signature) {
                        when (obj["signatureState"]?.jsonPrimitive?.contentOrNull) {
                            "empty" -> SignatureFieldState.Empty
                            "occupied" -> SignatureFieldState.Occupied
                            else -> SignatureFieldState.Unknown
                        }
                    } else {
                        null
                    },
            )
        }
    }
}
