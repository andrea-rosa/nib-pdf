package io.github.nibpdf.nib.ui.overlay

import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.focus.onFocusChanged
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.ui.text.input.TextFieldValue
import androidx.compose.ui.unit.IntOffset
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import io.github.nibpdf.nib.delta.FieldType
import io.github.nibpdf.nib.delta.FormField
import io.github.nibpdf.nib.delta.InvalidDeltaException
import io.github.nibpdf.nib.delta.Widget
import io.github.nibpdf.nib.render.TileKey
import io.github.nibpdf.nib.ui.NibPdfViewState
import io.github.nibpdf.nib.ui.toComposeColor
import io.github.nibpdf.nib.ui.toComposeRect
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.buildJsonArray
import kotlin.math.roundToInt

/**
 * Interactive AcroForm widgets for [page] in Form-tool mode.
 *
 * Every widget gets a positioned `Box` tagged `field:<name>` (`field:<name>:<export>`
 * for radio kids). A widget draws its frame opaque while the model value is
 * not yet visible in the flushed tile bytes (uncommitted, uncovered by
 * up-to-date tiles, or IME-focused); once tiles carry the baked `/AP` the box
 * turns transparent so the two aren't double-drawn.
 */
@Composable
@Suppress("ktlint:standard:function-naming")
internal fun FormOverlay(
    state: NibPdfViewState,
    page: Int,
    tileRevisions: Map<TileKey, Int>,
    modifier: Modifier = Modifier,
) {
    val version by state.document.annotationsVersion.collectAsState()
    val uncommitted by state.document.uncommittedIds.collectAsState()
    val fields =
        remember(version, page) {
            state.document.formFields().filter { f -> f.widgets.any { it.page == page } }
        }
    val t = state.transform(page)
    val k = t.k
    val density = LocalDensity.current.density
    val coverage =
        TileCoverage(
            page,
            state.bucket,
            density,
            state.document.pages[page],
            tileRevisions,
        )
    // Tab order = document field order (first text/choice widget per field).
    val editableNames =
        fields
            .filter { it.fieldType == FieldType.Text || (it.fieldType == FieldType.Combo && it.flags.editable) }
            .map { it.name }
    val requesters = remember(fields) { fields.associate { it.name to FocusRequester() } }

    Box(modifier.fillMaxSize()) {
        fields.forEach { field ->
            field.widgets.filter { it.page == page }.forEach { widget ->
                val r = t.rectToContent(widget.rect.toComposeRect())
                val opaque =
                    state.focusedField == field.name ||
                        field.name in uncommitted ||
                        !coverage.covered(
                            widget.rect,
                            state.document.commitRevisionOf(field.name),
                        )
                val tag =
                    if (field.fieldType == FieldType.Radio) {
                        "field:${field.name}:${widget.exportValue}"
                    } else {
                        "field:${field.name}"
                    }
                val boxMod =
                    Modifier
                        .offset { IntOffset(r.left.roundToInt(), r.top.roundToInt()) }
                        .size(
                            with(LocalDensity.current) { r.width.toDp() },
                            with(LocalDensity.current) { r.height.toDp() },
                        ).testTag(tag)
                        .semantics {
                            contentDescription = "widget:${field.fieldType.name.replaceFirstChar { it.lowercase() }}"
                        }.widgetFrame(widget, opaque, k, density)
                WidgetContent(
                    state,
                    field,
                    widget,
                    boxMod,
                    k,
                    density,
                    opaque,
                    requesters[field.name],
                    onNext = {
                        val i = editableNames.indexOf(field.name)
                        editableNames.getOrNull(i + 1)?.let { requesters[it]?.requestFocus() }
                    },
                    isLastEditable = editableNames.lastOrNull() == field.name,
                )
            }
        }
    }
}

private fun Modifier.widgetFrame(
    widget: Widget,
    opaque: Boolean,
    k: Float,
    density: Float,
): Modifier =
    if (opaque) {
        this
            .background((widget.backgroundColor ?: "#FFFFFF").toComposeColor())
            .border(
                width =
                    ((widget.borderWidth ?: 1f) * k / density)
                        .coerceAtLeast(0.5f)
                        .dp,
                color = (widget.borderColor ?: "#000000").toComposeColor(),
            )
    } else {
        this.background(Color.Transparent)
    }

private fun setValue(
    state: NibPdfViewState,
    field: FormField,
    value: kotlinx.serialization.json.JsonElement,
) {
    try {
        state.document.setFieldValue(field.name, value)
    } catch (_: InvalidDeltaException) {
        // Validation rejects (readOnly etc.); leave the widget as-is.
    }
}

@Composable
@Suppress("ktlint:standard:function-naming")
private fun WidgetContent(
    state: NibPdfViewState,
    field: FormField,
    widget: Widget,
    boxMod: Modifier,
    k: Float,
    density: Float,
    opaque: Boolean,
    requester: FocusRequester?,
    onNext: () -> Unit,
    isLastEditable: Boolean,
) {
    when (field.fieldType) {
        FieldType.Text ->
            TextWidget(
                state,
                field,
                boxMod,
                k,
                density,
                opaque,
                requester,
                onNext,
                isLastEditable,
            )
        FieldType.Checkbox -> {
            val export = widget.exportValue ?: field.exportValues.firstOrNull() ?: "Yes"
            val on = field.valueString == export || widget.appearanceState == export
            Box(
                boxMod.clickable(enabled = !field.flags.readOnly) {
                    setValue(state, field, JsonPrimitive(if (on) "Off" else export))
                },
            ) {
                if (on && opaque) {
                    Canvas(Modifier.fillMaxSize()) {
                        val w = size.width
                        val h = size.height
                        val p = Path()
                        p.moveTo(w * 0.2f, h * 0.55f)
                        p.lineTo(w * 0.42f, h * 0.75f)
                        p.lineTo(w * 0.8f, h * 0.28f)
                        drawPath(p, Color(0xFF1A237E), style = Stroke(w * 0.09f))
                    }
                }
            }
        }
        FieldType.Radio -> {
            val export = widget.exportValue ?: "On"
            val on = field.valueString == export || widget.appearanceState == export
            Box(
                boxMod.clickable(enabled = !field.flags.readOnly) {
                    val next =
                        if (on && !field.flags.noToggleToOff) "Off" else export
                    setValue(state, field, JsonPrimitive(next))
                },
            ) {
                if (opaque) {
                    Canvas(Modifier.fillMaxSize()) {
                        val r = minOf(size.width, size.height) / 2f
                        drawCircle(Color(0xFF1A237E), radius = r * 0.9f, style = Stroke(r * 0.14f))
                        if (on) drawCircle(Color(0xFF1A237E), radius = r * 0.55f)
                    }
                }
            }
        }
        FieldType.Combo ->
            ChoiceWidget(
                state,
                field,
                boxMod,
                k,
                density,
                opaque,
                multi = false,
                editable = field.flags.editable,
                requester,
                onNext,
                isLastEditable,
            )
        FieldType.List ->
            ChoiceWidget(
                state,
                field,
                boxMod,
                k,
                density,
                opaque,
                multi = field.flags.multiSelect,
                editable = false,
                requester,
                onNext,
                isLastEditable,
            )
        FieldType.PushButton, FieldType.Signature -> Box(boxMod)
    }
}

@Composable
@Suppress("ktlint:standard:function-naming")
private fun TextWidget(
    state: NibPdfViewState,
    field: FormField,
    boxMod: Modifier,
    k: Float,
    density: Float,
    opaque: Boolean,
    requester: FocusRequester?,
    onNext: () -> Unit,
    isLastEditable: Boolean,
) {
    if (!opaque) {
        // Covered by the tile bytes: pure transparent hit-target. Tapping
        // focuses the field, which flips `opaque` and reveals the editor.
        Box(
            boxMod.clickable(enabled = !field.flags.readOnly) {
                state.focusedField = field.name
            },
        )
        return
    }
    // The BasicTextField only exists while opaque; ask it for focus once
    // this field became the focused one (direct tap or tab traversal).
    androidx.compose.runtime.LaunchedEffect(state.focusedField, requester) {
        if (state.focusedField == field.name && requester != null) {
            requester.requestFocus()
        }
    }
    var tfv by remember(field.name) { mutableStateOf(TextFieldValue(field.valueString ?: "")) }
    var focused by remember { mutableStateOf(false) }
    // External changes (undo, import) while unfocused resync the editor.
    val external = field.valueString ?: ""
    androidx.compose.runtime.LaunchedEffect(external) {
        if (!focused) tfv = TextFieldValue(external)
    }
    val fontSp =
        (
            if ((field.fontSize ?: 0f) > 0f) {
                (field.fontSize ?: 12f) * k / density
            } else {
                // Fitted: ~60% of widget height, clamped to a readable range.
                val hPt =
                    field.widgets
                        .firstOrNull()
                        ?.rect
                        ?.height() ?: 16f
                (hPt * 0.6f * k / density).coerceIn(6f, 24f)
            }
        ).sp
    Box(boxMod.padding(horizontal = with(LocalDensity.current) { (2f * k).toDp() })) {
        BasicTextField(
            value = tfv,
            onValueChange = { nv ->
                val max = field.maxLen
                if (max == null || nv.text.length <= max) {
                    tfv = nv
                    setValue(state, field, JsonPrimitive(nv.text))
                }
            },
            modifier =
                Modifier
                    .fillMaxSize()
                    .let { if (requester != null) it.focusRequester(requester) else it }
                    .onFocusChanged {
                        if (it.isFocused) {
                            state.focusedField = field.name
                        } else if (focused && state.focusedField == field.name) {
                            state.focusedField = null
                        }
                        focused = it.isFocused
                    },
            enabled = !field.flags.readOnly,
            singleLine = !field.flags.multiline,
            textStyle = TextStyle(fontSize = fontSp),
            keyboardOptions =
                KeyboardOptions(
                    imeAction = if (isLastEditable) ImeAction.Done else ImeAction.Next,
                ),
            keyboardActions = KeyboardActions(onNext = { onNext() }),
            visualTransformation =
                if (field.flags.password) PasswordVisualTransformation() else androidx.compose.ui.text.input.VisualTransformation.None,
        )
    }
}

@Composable
@Suppress("ktlint:standard:function-naming")
private fun ChoiceWidget(
    state: NibPdfViewState,
    field: FormField,
    boxMod: Modifier,
    k: Float,
    density: Float,
    opaque: Boolean,
    multi: Boolean,
    editable: Boolean,
    requester: FocusRequester?,
    onNext: () -> Unit,
    isLastEditable: Boolean,
) {
    var menuOpen by remember { mutableStateOf(false) }
    val selected = field.valueList.toSet()
    Box(boxMod) {
        if (editable) {
            TextWidget(state, field, Modifier.fillMaxSize(), k, density, opaque, requester, onNext, isLastEditable)
        } else {
            Box(
                Modifier
                    .fillMaxSize()
                    .clickable(enabled = !field.flags.readOnly) { menuOpen = true },
            ) {
                if (opaque) {
                    val label =
                        field.options.firstOrNull { it.export in selected }?.display
                            ?: field.valueString.orEmpty()
                    Text(
                        label,
                        modifier = Modifier.align(Alignment.CenterStart),
                        fontSize =
                            (
                                if ((field.fontSize ?: 0f) > 0f) {
                                    (field.fontSize ?: 12f) * k / density
                                } else {
                                    12f
                                }
                            ).sp,
                    )
                }
            }
        }
        DropdownMenu(expanded = menuOpen, onDismissRequest = { menuOpen = false }) {
            field.options.forEach { opt ->
                DropdownMenuItem(
                    text = {
                        val checked = opt.export in selected
                        Text("${if (checked) "✓ " else ""}${opt.display}")
                    },
                    onClick = {
                        if (multi) {
                            val next =
                                if (opt.export in selected) {
                                    field.valueList - opt.export
                                } else {
                                    field.valueList + opt.export
                                }
                            setValue(
                                state,
                                field,
                                buildJsonArray { next.forEach { add(JsonPrimitive(it)) } },
                            )
                        } else {
                            setValue(state, field, JsonPrimitive(opt.export))
                            menuOpen = false
                        }
                    },
                )
            }
        }
    }
}
