package io.github.nibpdf.nib.delta

import kotlinx.serialization.json.jsonObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class FormFieldParsingTest {
    private fun field(raw: String): FormField = FormField.fromJson(NibJson.parseToJsonElement(raw).jsonObject)

    @Test
    fun checkboxFieldParsesFlagsAndWidget() {
        val f =
            field(
                """
                {
                  "name": "agree",
                  "fieldType": "checkbox",
                  "value": "Yes",
                  "flags": { "required": true, "noToggleToOff": true },
                  "exportValues": ["Yes"],
                  "widgets": [
                    {
                      "id": "w1",
                      "page": 0,
                      "rect": [10.0, 20.0, 30.0, 40.0],
                      "appearanceState": "Yes",
                      "exportValue": "Yes",
                      "borderColor": "#000000",
                      "backgroundColor": "#FFFFFF",
                      "borderWidth": 1.0
                    }
                  ]
                }
                """.trimIndent(),
            )
        assertEquals("agree", f.name)
        assertEquals(FieldType.Checkbox, f.fieldType)
        assertEquals("Yes", f.valueString)
        assertEquals(listOf("Yes"), f.valueList)
        assertTrue(f.flags.required)
        assertTrue(f.flags.noToggleToOff)
        assertFalse(f.flags.readOnly)
        assertEquals(listOf("Yes"), f.exportValues)
        assertEquals(1, f.widgets.size)
        val w = f.widgets[0]
        assertEquals("w1", w.id)
        assertEquals(0, w.page)
        // RectF is an android.jar stub in JVM tests — its numbers can't be
        // asserted here; parsing is covered by the on-device tests.
        assertEquals("Yes", w.appearanceState)
        assertEquals("Yes", w.exportValue)
        assertEquals("#FFFFFF", w.backgroundColor)
        assertEquals(1f, w.borderWidth)
    }

    @Test
    fun comboFieldParsesOptionsAndAppearance() {
        val f =
            field(
                """
                {
                  "name": "country",
                  "fieldType": "combo",
                  "value": "IT",
                  "maxLen": 8,
                  "textAlign": "center",
                  "defaultAppearance": { "fontSize": 11.0, "color": "#123456" },
                  "flags": { "readOnly": true, "editable": true },
                  "options": [
                    { "export": "IT", "label": "Italy" },
                    { "export": "DE", "label": "Germany" }
                  ],
                  "topIndex": 2,
                  "widgets": [
                    { "page": 0, "rect": [0.0, 0.0, 100.0, 20.0] }
                  ]
                }
                """.trimIndent(),
            )
        assertEquals(FieldType.Combo, f.fieldType)
        assertEquals(8, f.maxLen)
        assertTrue(f.flags.readOnly)
        assertTrue(f.flags.editable)
        assertEquals("center", f.textAlign)
        assertEquals(11f, f.fontSize)
        assertEquals("#123456", f.textColor)
        assertEquals(listOf("IT", "DE"), f.options.map { it.export })
        assertEquals("Italy", f.options[0].label)
        assertEquals("Germany", f.options[1].display)
        assertEquals(2, f.topIndex)
        assertNull(f.widgets[0].exportValue)
    }

    @Test
    fun multiSelectListValueList() {
        val f =
            field(
                """
                {
                  "name": "plan",
                  "fieldType": "list",
                  "value": ["Pro", "Enterprise"],
                  "flags": { "multiSelect": true },
                  "widgets": []
                }
                """.trimIndent(),
            )
        assertEquals(listOf("Pro", "Enterprise"), f.valueList)
        assertNull(f.valueString)
        assertTrue(f.flags.multiSelect)
        assertTrue(f.widgets.isEmpty())
    }
}
