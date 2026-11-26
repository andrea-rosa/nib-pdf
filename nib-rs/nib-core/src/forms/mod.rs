//! AcroForm field-tree parser (JSON_SCHEMA §7). Walks `/Root /AcroForm
//! /Fields` depth-first into `FormField` entities: `/T` chains give
//! fully-qualified names, field keys (`/FT /Ff /DA /Q /V /DV /Opt /MaxLen
//! /TI /I`) inherit through `/Parent` (depth cap 32), and each widget kid
//! (or the merged field+widget dict) becomes a `Widget`.

use std::collections::{HashMap, HashSet};

use lopdf::{Dictionary, Document, Object, ObjectId};
use serde_json::Value;
use uuid::Uuid;

use crate::model::{
    Color, DefaultAppearance, FieldFlags, FieldOption, FieldType, FormField, SignatureFieldState,
    TextAlign, Widget,
};
use crate::writer::{dict, NIB_NS};

/// Where a parsed field lives in the file: the field dict plus every widget
/// object (with its page index).
#[derive(Debug, Clone)]
pub struct FieldRefs {
    pub field: ObjectId,
    pub widgets: Vec<(ObjectId, u32)>,
    /// True when the field dict is itself the (only) widget annotation.
    pub kids_are_merged: bool,
}

#[derive(Debug, Default)]
pub struct FormImport {
    pub fields: Vec<FormField>,
    pub refs: HashMap<String, FieldRefs>,
    /// `/AcroForm` object id, when it is an indirect object.
    pub acroform: Option<ObjectId>,
    /// `/AcroForm` is an inline dict inside the catalog.
    pub acroform_inline: bool,
    pub need_appearances: bool,
    pub warnings: Vec<String>,
}

const INHERIT_DEPTH: u32 = 32;
const FIELD_DEPTH: usize = 64;

/// Field keys that inherit through `/Parent` (ISO 32000-1 §12.7.3.3).
const INHERITABLE: &[&[u8]] = &[
    b"FT", b"Ff", b"DA", b"Q", b"V", b"DV", b"Opt", b"MaxLen", b"TI", b"I",
];

pub fn parse(doc: &Document, page_ids: &[ObjectId]) -> FormImport {
    let mut out = FormImport::default();
    let Ok(catalog) = doc.catalog() else {
        return out;
    };
    let Ok(af) = catalog.get(b"AcroForm") else {
        return out;
    };
    let acro: Dictionary = match af {
        Object::Reference(r) => {
            out.acroform = Some(*r);
            match doc.get_object(*r).and_then(|o| o.as_dict()) {
                Ok(d) => d.clone(),
                Err(_) => return out,
            }
        }
        Object::Dictionary(d) => {
            out.acroform_inline = true;
            d.clone()
        }
        _ => return out,
    };
    out.need_appearances = acro
        .get(b"NeedAppearances")
        .ok()
        .and_then(|o| o.as_bool().ok())
        .unwrap_or(false);

    // Widget oid → page index, from each page's /Annots.
    let mut widget_page: HashMap<ObjectId, u32> = HashMap::new();
    for (i, &page_id) in page_ids.iter().enumerate() {
        let entries = doc
            .get_object(page_id)
            .and_then(|o| o.as_dict())
            .ok()
            .and_then(|p| p.get(b"Annots").ok())
            .and_then(|a| match a {
                Object::Array(v) => Some(v.clone()),
                Object::Reference(r) => doc
                    .get_object(*r)
                    .ok()
                    .and_then(|o| o.as_array().ok().cloned()),
                _ => None,
            })
            .unwrap_or_default();
        for e in entries {
            if let Object::Reference(r) = e {
                widget_page.insert(r, i as u32);
            }
        }
    }

    let roots: Vec<ObjectId> = acro
        .get(b"Fields")
        .ok()
        .and_then(|o| match o {
            Object::Array(v) => Some(v.clone()),
            Object::Reference(r) => doc
                .get_object(*r)
                .ok()
                .and_then(|o| o.as_array().ok().cloned()),
            _ => None,
        })
        .unwrap_or_default()
        .into_iter()
        .filter_map(|o| o.as_reference().ok())
        .collect();

    let mut walker = FieldWalker {
        doc,
        page_ids,
        widget_page: &widget_page,
        visited_fields: HashSet::new(),
    };
    for root in roots {
        walk(&mut walker, root, String::new(), 0, &mut out);
    }
    let mut counts = HashMap::new();
    for field in &out.fields {
        *counts.entry(field.name.clone()).or_insert(0usize) += 1;
    }
    let ambiguous: HashSet<String> = out
        .fields
        .iter()
        .filter(|field| {
            field.field_type == FieldType::Signature
                && counts.get(&field.name).copied().unwrap_or(0) > 1
        })
        .map(|field| field.name.clone())
        .collect();
    for field in &mut out.fields {
        if field.field_type == FieldType::Signature && ambiguous.contains(&field.name) {
            field.signature_state = Some(SignatureFieldState::Unknown);
        }
    }
    for name in ambiguous {
        out.warnings
            .push(format!("signature field {name}: duplicate field name"));
    }
    out
}

fn dict_of(doc: &Document, oid: ObjectId) -> Option<Dictionary> {
    doc.get_object(oid)
        .ok()
        .and_then(|o| o.as_dict().ok().cloned())
}

fn text_of(o: Option<&Object>) -> Option<String> {
    match o? {
        s @ Object::String(..) => lopdf::decode_text_string(s).ok(),
        Object::Name(n) => Some(String::from_utf8_lossy(n).into_owned()),
        _ => None,
    }
}

/// Dereferences `o` one level (references to dicts/arrays/strings).
fn deref<'a>(doc: &'a Document, o: &'a Object) -> &'a Object {
    match o {
        Object::Reference(r) => doc.get_object(*r).unwrap_or(o),
        _ => o,
    }
}

/// `dict[key]`, falling back through `/Parent` (depth-capped).
fn inherited(doc: &Document, d: &Dictionary, key: &[u8]) -> Option<Object> {
    let mut cur = Some(d.clone());
    let mut depth = 0;
    while let Some(d) = cur {
        if let Ok(v) = d.get(key) {
            return Some(v.clone());
        }
        if !INHERITABLE.contains(&key) || depth >= INHERIT_DEPTH {
            return None;
        }
        depth += 1;
        cur = match d.get(b"Parent") {
            Ok(Object::Reference(r)) => dict_of(doc, *r),
            _ => None,
        };
    }
    None
}

struct FieldWalker<'a> {
    doc: &'a Document,
    page_ids: &'a [ObjectId],
    widget_page: &'a HashMap<ObjectId, u32>,
    visited_fields: HashSet<ObjectId>,
}

fn walk(
    walker: &mut FieldWalker<'_>,
    oid: ObjectId,
    prefix: String,
    depth: usize,
    out: &mut FormImport,
) {
    if depth > FIELD_DEPTH {
        out.warnings.push(format!(
            "field {oid:?} exceeds maximum field depth {FIELD_DEPTH}"
        ));
        return;
    }
    if !walker.visited_fields.insert(oid) {
        out.warnings
            .push(format!("field {oid:?} has a cycle or repeated reference"));
        return;
    }

    let Some(d) = dict_of(walker.doc, oid) else {
        out.warnings.push(format!("field {oid:?} unresolvable"));
        return;
    };
    let local = text_of(d.get(b"T").ok()).unwrap_or_default();
    let name = if prefix.is_empty() {
        local
    } else {
        format!("{prefix}.{local}")
    };

    let kids: Vec<(ObjectId, Dictionary)> = d
        .get(b"Kids")
        .ok()
        .and_then(|o| match o {
            Object::Array(v) => Some(v.clone()),
            Object::Reference(r) => walker
                .doc
                .get_object(*r)
                .ok()
                .and_then(|o| o.as_array().ok().cloned()),
            _ => None,
        })
        .unwrap_or_default()
        .into_iter()
        .filter_map(|o| {
            o.as_reference()
                .ok()
                .and_then(|r| dict_of(walker.doc, r).map(|d| (r, d)))
        })
        .collect();
    let (field_kids, widget_kids): (Vec<_>, Vec<_>) =
        kids.into_iter().partition(|(_, kd)| kd.get(b"T").is_ok());

    if !field_kids.is_empty() {
        // Non-terminal node: recurse; stray widget kids are unusable.
        if !widget_kids.is_empty() {
            out.warnings
                .push(format!("widget kids of non-terminal field {name} skipped"));
        }
        for (kid, _) in field_kids {
            walk(walker, kid, name.clone(), depth + 1, out);
        }
        return;
    }

    if name.is_empty() {
        out.warnings.push(format!("field {oid:?} has no /T"));
        return;
    }

    let widget_ids: Vec<(ObjectId, Dictionary)> = if widget_kids.is_empty() {
        // Merged field+widget dict (self is the widget) or widget-less field.
        let is_widget = d.get(b"Subtype").ok().and_then(dict::name_of).as_deref() == Some("Widget")
            || d.get(b"Rect").is_ok();
        if is_widget {
            vec![(oid, d.clone())]
        } else {
            Vec::new()
        }
    } else {
        widget_kids
    };

    let field_flags = inherited(walker.doc, &d, b"Ff");
    let ff_bits = field_flags
        .as_ref()
        .and_then(|o| dict::num(deref(walker.doc, o)))
        .unwrap_or(0.0) as i64;
    let ft_name = inherited(walker.doc, &d, b"FT").and_then(|o| dict::name_of(&o));
    let field_type = match ft_name.as_deref() {
        Some("Btn") => {
            if ff_bits & (1 << 16) != 0 {
                FieldType::PushButton
            } else if ff_bits & (1 << 15) != 0 {
                FieldType::Radio
            } else {
                FieldType::Checkbox
            }
        }
        Some("Ch") => {
            if ff_bits & (1 << 17) != 0 {
                FieldType::Combo
            } else {
                FieldType::List
            }
        }
        Some("Sig") => FieldType::Signature,
        Some("Tx") | None => FieldType::Text,
        Some(other) => {
            out.warnings
                .push(format!("field {name}: unknown /FT {other}"));
            FieldType::Text
        }
    };

    let mut flags = FieldFlags {
        read_only: ff_bits & 1 != 0,
        required: ff_bits & 2 != 0,
        no_export: ff_bits & 4 != 0,
        ..FieldFlags::default()
    };
    match field_type {
        FieldType::Text => {
            flags.multiline = ff_bits & (1 << 12) != 0;
            flags.password = ff_bits & (1 << 13) != 0;
            flags.do_not_spell_check = ff_bits & (1 << 22) != 0;
            flags.do_not_scroll = ff_bits & (1 << 23) != 0;
            flags.comb = ff_bits & (1 << 24) != 0;
            flags.rich_text = ff_bits & (1 << 25) != 0;
        }
        FieldType::Checkbox | FieldType::Radio | FieldType::PushButton => {
            flags.no_toggle_to_off = ff_bits & (1 << 14) != 0;
            flags.radios_in_unison = ff_bits & (1 << 25) != 0;
        }
        FieldType::Combo | FieldType::List => {
            flags.editable = ff_bits & (1 << 18) != 0;
            flags.sort = ff_bits & (1 << 19) != 0;
            flags.multi_select = ff_bits & (1 << 21) != 0;
            flags.do_not_spell_check = ff_bits & (1 << 22) != 0;
            flags.do_not_scroll = ff_bits & (1 << 23) != 0;
        }
        _ => {}
    }

    // /AP /N appearance-state keys of one widget dict (non-"Off", sorted).
    fn ap_states(doc: &Document, w: &Dictionary) -> Vec<String> {
        let ap = match w.get(b"AP") {
            Ok(o) => deref(doc, o),
            Err(_) => return Vec::new(),
        };
        let Ok(apd) = ap.as_dict() else {
            return Vec::new();
        };
        let n = match apd.get(b"N") {
            Ok(o) => deref(doc, o),
            Err(_) => return Vec::new(),
        };
        let Ok(nd) = n.as_dict() else {
            return Vec::new();
        };
        nd.iter()
            .filter_map(|(k, _)| {
                let k = String::from_utf8_lossy(k).into_owned();
                (k != "Off").then_some(k)
            })
            .collect()
    }

    let mut widgets = Vec::new();
    let mut widget_refs = Vec::new();
    let mut export_values: Vec<String> = Vec::new();
    for (woid, wd) in &widget_ids {
        for st in ap_states(walker.doc, wd) {
            if !export_values.contains(&st) {
                export_values.push(st);
            }
        }
        let page = walker.widget_page.get(woid).copied().or_else(|| {
            wd.get(b"P")
                .ok()
                .and_then(|o| o.as_reference().ok())
                .and_then(|p| walker.page_ids.iter().position(|id| *id == p))
                .map(|i| i as u32)
        });
        let (Some(page), Some(rect)) = (page, dict::rect_of(wd.get(b"Rect").ok())) else {
            out.warnings
                .push(format!("widget {woid:?} of {name}: no page/rect, skipped"));
            continue;
        };
        let export_value = ap_states(walker.doc, wd).into_iter().next();
        let mk = wd
            .get(b"MK")
            .ok()
            .and_then(|o| deref(walker.doc, o).as_dict().ok());
        let bs = wd
            .get(b"BS")
            .ok()
            .and_then(|o| deref(walker.doc, o).as_dict().ok());
        widgets.push(Widget {
            id: Some(
                Uuid::new_v5(&NIB_NS, format!("widget:{}:{}", woid.0, woid.1).as_bytes())
                    .to_string(),
            ),
            page,
            rect,
            appearance_state: wd.get(b"AS").ok().and_then(dict::name_of),
            export_value,
            border_color: mk.and_then(|m| dict::color_of(m.get(b"BC").ok())),
            background_color: mk.and_then(|m| dict::color_of(m.get(b"BG").ok())),
            border_width: Some(
                bs.and_then(|b| b.get(b"W").ok())
                    .and_then(dict::num)
                    .unwrap_or(1.0),
            ),
            border_style: dict::border_style_of(bs),
            rotation: mk.and_then(|m| m.get(b"R").ok()).and_then(dict::num),
        });
        widget_refs.push((*woid, page));
    }

    // Radios may carry export values in /Opt instead of /AP /N keys.
    let options = options_of(walker.doc, &inherited(walker.doc, &d, b"Opt"));
    if matches!(field_type, FieldType::Radio) && export_values.is_empty() {
        if let Some(opts) = &options {
            export_values = opts.iter().map(|o| o.export.clone()).collect();
        }
    }

    let value = field_value(walker.doc, &inherited(walker.doc, &d, b"V"), field_type);
    let default_value = field_value(walker.doc, &inherited(walker.doc, &d, b"DV"), field_type);
    let mk = d
        .get(b"MK")
        .ok()
        .and_then(|o| deref(walker.doc, o).as_dict().ok());
    let field = FormField {
        name: name.clone(),
        field_type,
        signature_state: (field_type == FieldType::Signature).then(|| {
            if field_flags.as_ref().is_some_and(
                |value| !matches!(deref(walker.doc, value), Object::Integer(bits) if (0..=u32::MAX as i64).contains(bits)),
            ) {
                SignatureFieldState::Unknown
            } else {
                signature_field_state(walker.doc, &d)
            }
        }),
        value,
        default_value,
        flags,
        max_len: inherited(walker.doc, &d, b"MaxLen")
            .and_then(|o| dict::num(&o))
            .map(|n| n.max(0.0) as u32),
        default_appearance: parse_da(inherited(walker.doc, &d, b"DA").as_ref()),
        text_align: inherited(walker.doc, &d, b"Q")
            .and_then(|o| dict::num(&o))
            .map(|q| match q as i64 {
                1 => TextAlign::Center,
                2 => TextAlign::Right,
                _ => TextAlign::Left,
            }),
        tooltip: inherited(walker.doc, &d, b"TU")
            .and_then(|o| text_of(Some(deref(walker.doc, &o)))),
        export_values: (!export_values.is_empty()).then_some(export_values),
        options,
        top_index: inherited(walker.doc, &d, b"TI")
            .and_then(|o| dict::num(&o))
            .map(|n| n.max(0.0) as u32),
        caption: mk.and_then(|m| text_of(m.get(b"CA").ok())),
        widgets,
        extra: Default::default(),
    };
    out.refs.insert(
        name.clone(),
        FieldRefs {
            field: oid,
            widgets: widget_refs.clone(),
            kids_are_merged: widget_refs.len() == 1 && widget_refs[0].0 == oid,
        },
    );
    out.fields.push(field);
}

fn options_of(doc: &Document, opt: &Option<Object>) -> Option<Vec<FieldOption>> {
    let arr = match opt.as_ref()? {
        o @ Object::Array(..) => o.as_array().ok()?,
        o @ Object::Reference(..) => deref(doc, o).as_array().ok()?,
        _ => return None,
    };
    let mut out = Vec::new();
    for e in arr {
        match deref(doc, e) {
            Object::Array(pair) => {
                if let Some(export) = pair.first().and_then(|o| text_of(Some(o))) {
                    let label = pair.get(1).and_then(|o| text_of(Some(o)));
                    out.push(FieldOption { export, label });
                }
            }
            other => {
                if let Some(export) = text_of(Some(other)) {
                    out.push(FieldOption {
                        export,
                        label: None,
                    });
                }
            }
        }
    }
    Some(out)
}

fn field_value(doc: &Document, v: &Option<Object>, ft: FieldType) -> Option<Value> {
    let o = deref(doc, v.as_ref()?);
    match ft {
        FieldType::Signature => None,
        FieldType::Checkbox | FieldType::Radio | FieldType::PushButton => Some(match o {
            Object::Name(n) => Value::String(String::from_utf8_lossy(n).into_owned()),
            _ => Value::String("Off".into()),
        }),
        FieldType::Combo | FieldType::List => match o {
            Object::Array(a) => Some(Value::Array(
                a.iter()
                    .filter_map(|x| text_of(Some(deref(doc, x))).map(Value::String))
                    .collect(),
            )),
            _ => text_of(Some(o)).map(Value::String),
        },
        _ => text_of(Some(o)).map(Value::String),
    }
}

fn signature_field_state(doc: &Document, field: &Dictionary) -> SignatureFieldState {
    let mut current = field.clone();
    let mut parents = HashSet::new();
    for _ in 0..=INHERIT_DEPTH {
        if let Ok(value) = current.get(b"V") {
            let mut value = value;
            let mut references = HashSet::new();
            let mut null_value = false;
            for _ in 0..=INHERIT_DEPTH {
                match value {
                    Object::Null => {
                        null_value = true;
                        break;
                    }
                    Object::Reference(id) if references.insert(*id) => {
                        let Ok(resolved) = doc.get_object(*id) else {
                            return SignatureFieldState::Unknown;
                        };
                        value = resolved;
                    }
                    Object::Dictionary(d) if d.has(b"ByteRange") => {
                        return SignatureFieldState::Occupied;
                    }
                    _ => return SignatureFieldState::Unknown,
                }
            }
            if !null_value {
                return SignatureFieldState::Unknown;
            }
        }
        match current.get(b"Parent") {
            Err(_) => return SignatureFieldState::Empty,
            Ok(Object::Reference(id)) if parents.insert(*id) => {
                let Some(parent) = dict_of(doc, *id) else {
                    return SignatureFieldState::Unknown;
                };
                current = parent;
            }
            _ => return SignatureFieldState::Unknown,
        }
    }
    SignatureFieldState::Unknown
}

/// `/DA` → `DefaultAppearance`: `/Name size Tf` + `g`/`rg`/`k` colour ops.
fn parse_da(o: Option<&Object>) -> Option<DefaultAppearance> {
    let s = text_of(o)?;
    let toks: Vec<&str> = s.split_whitespace().collect();
    let mut family = None;
    let mut size = None;
    let mut color = None;
    let num_at = |i: usize| toks.get(i).and_then(|t| t.parse::<f64>().ok());
    let to_hex = |v: &[f64]| {
        let c = |x: f64| format!("{:02X}", (x.clamp(0.0, 1.0) * 255.0).round() as u8);
        Color(format!("#{}{}{}", c(v[0]), c(v[1]), c(v[2])))
    };
    for (i, t) in toks.iter().enumerate() {
        if *t == "Tf" && i >= 2 {
            let name = toks[i - 2].trim_start_matches('/');
            family = Some(
                match name {
                    "Helv" => "Helvetica",
                    "HeBo" => "Helvetica-Bold",
                    "HeOb" => "Helvetica-Oblique",
                    "HeBO" => "Helvetica-BoldOblique",
                    "TiRo" => "Times-Roman",
                    "TiBo" => "Times-Bold",
                    "TiIt" => "Times-Italic",
                    "TiBI" => "Times-BoldItalic",
                    "Cour" => "Courier",
                    "CoBo" => "Courier-Bold",
                    "CoOb" => "Courier-Oblique",
                    "CoBO" => "Courier-BoldOblique",
                    "ZaDb" => "ZapfDingbats",
                    n => n,
                }
                .to_string(),
            );
            size = num_at(i - 1);
        }
        let operands = |n: usize| -> Option<Vec<f64>> {
            if i < n {
                return None;
            }
            toks[i - n..i]
                .iter()
                .map(|t| t.parse::<f64>().ok())
                .collect()
        };
        match *t {
            "rg" => {
                if let Some(v) = operands(3) {
                    color = Some(to_hex(&v));
                }
            }
            "g" => {
                if let Some(v) = operands(1) {
                    color = Some(to_hex(&[v[0], v[0], v[0]]));
                }
            }
            "k" => {
                if let Some(v) = operands(4) {
                    color = Some(to_hex(&[
                        (1.0 - v[0]).max(0.0) * (1.0 - v[3]),
                        (1.0 - v[1]).max(0.0) * (1.0 - v[3]),
                        (1.0 - v[2]).max(0.0) * (1.0 - v[3]),
                    ]));
                }
            }
            _ => {}
        }
    }
    if family.is_none() && size.is_none() && color.is_none() {
        return None;
    }
    Some(DefaultAppearance {
        font_family: family,
        font_size: size,
        color,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use lopdf::{dictionary, Stream};
    use std::path::Path;

    fn fixture() -> Document {
        Document::load(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/form.pdf"))
            .unwrap()
    }

    fn page_ids(doc: &Document) -> Vec<ObjectId> {
        doc.get_pages().values().copied().collect()
    }

    /// Minimal in-memory doc: one page, catalog → `/AcroForm << /Fields … >>`.
    /// Returns (doc, page_id); callers add objects then call `finish`.
    struct Synth {
        doc: Document,
        page: ObjectId,
        annots: Vec<ObjectId>,
        fields: Vec<ObjectId>,
    }

    impl Synth {
        fn new() -> Self {
            let mut doc = Document::new();
            let page = doc.add_object(dictionary! {
                "Type" => "Page",
                "MediaBox" => vec![Object::from(0.0), Object::from(0.0), Object::from(612.0), Object::from(792.0)],
            });
            Synth {
                doc,
                page,
                annots: Vec::new(),
                fields: Vec::new(),
            }
        }

        fn add(&mut self, d: Dictionary) -> ObjectId {
            self.doc.add_object(Object::Dictionary(d))
        }

        fn finish(mut self) -> (Document, Vec<ObjectId>) {
            let page_dict = self
                .doc
                .get_object_mut(self.page)
                .unwrap()
                .as_dict_mut()
                .unwrap();
            page_dict.set(
                "Annots",
                self.annots
                    .iter()
                    .map(|i| Object::Reference(*i))
                    .collect::<Vec<_>>(),
            );
            let pages = self.doc.add_object(dictionary! {
                "Type" => "Pages",
                "Kids" => vec![Object::Reference(self.page)],
                "Count" => 1,
            });
            let page = self.page;
            self.doc
                .get_object_mut(page)
                .unwrap()
                .as_dict_mut()
                .unwrap()
                .set("Parent", Object::Reference(pages));
            let acro = self.doc.add_object(dictionary! {
                "Fields" => self.fields.iter().map(|i| Object::Reference(*i)).collect::<Vec<_>>(),
            });
            let catalog = self.doc.add_object(dictionary! {
                "Type" => "Catalog",
                "Pages" => Object::Reference(pages),
                "AcroForm" => Object::Reference(acro),
            });
            self.doc.trailer.set("Root", Object::Reference(catalog));
            (self.doc, vec![page])
        }
    }

    fn ap_n(doc: &mut Document, rect: [f64; 4], export: &str) -> Dictionary {
        let w = rect[2] - rect[0];
        let h = rect[3] - rect[1];
        let mk = |doc: &mut Document| {
            doc.add_object(Object::Stream(Stream::new(
                dictionary! {
                    "Type" => "XObject",
                    "Subtype" => "Form",
                    "FormType" => 1,
                    "BBox" => vec![
                        Object::from(0.0), Object::from(0.0),
                        Object::from(w), Object::from(h),
                    ],
                },
                b"q\nQ\n".to_vec(),
            )))
        };
        let on = mk(doc);
        let off = mk(doc);
        dictionary! {
            "N" => dictionary! {
                export => Object::Reference(on),
                "Off" => Object::Reference(off),
            },
        }
    }

    #[test]
    fn fixture_form_parses_twelve_fields() {
        let doc = fixture();
        let imp = parse(&doc, &page_ids(&doc));
        assert_eq!(imp.fields.len(), 12, "warnings: {:?}", imp.warnings);
        let names: Vec<&str> = imp.fields.iter().map(|f| f.name.as_str()).collect();
        for want in [
            "name",
            "notes",
            "code",
            "secret",
            "readonly",
            "agree",
            "newsletter",
            "color",
            "country",
            "city",
            "plan",
            "bottom.note",
        ] {
            assert!(names.contains(&want), "missing {want}: {names:?}");
        }
        let by_name = |n: &str| imp.fields.iter().find(|f| f.name == n).unwrap();
        assert_eq!(by_name("name").field_type, FieldType::Text);
        assert_eq!(by_name("agree").field_type, FieldType::Checkbox);
        assert_eq!(by_name("color").field_type, FieldType::Radio);
        assert_eq!(by_name("country").field_type, FieldType::Combo);
        assert_eq!(by_name("plan").field_type, FieldType::List);
        assert!(by_name("notes").flags.multiline);
        assert!(by_name("code").flags.comb);
        assert!(by_name("secret").flags.password);
        assert!(by_name("readonly").flags.read_only);
        assert_eq!(
            by_name("readonly").value,
            Some(Value::String("fixed".into()))
        );
        assert_eq!(by_name("agree").value, Some(Value::String("Off".into())));
        let opts = by_name("country").options.clone().unwrap();
        assert_eq!(opts.len(), 3);
        assert_eq!(opts[0].export, "IT");
        assert_eq!(opts[0].label.as_deref(), Some("Italy"));
        assert_eq!(opts[2].export, "FR");
        assert_eq!(
            by_name("color").export_values,
            Some(vec!["Red".into(), "Green".into(), "Blue".into()])
        );
        for f in &imp.fields {
            for w in &f.widgets {
                assert_eq!(w.page, 0, "{} widget page", f.name);
            }
        }
        let agree = by_name("agree");
        assert_eq!(agree.widgets.len(), 1);
        assert_eq!(agree.widgets[0].export_value.as_deref(), Some("Yes"));
        assert_eq!(agree.widgets[0].appearance_state.as_deref(), Some("Off"));
        assert_eq!(by_name("color").widgets.len(), 3);
        assert!(imp.need_appearances);
        assert!(imp.acroform.is_some());
        assert!(!imp.acroform_inline);
        // /DA inheritance: bottom.note inherits the parent's /DA.
        assert!(by_name("bottom.note").default_appearance.is_some());
        assert_eq!(
            by_name("notes")
                .default_appearance
                .as_ref()
                .unwrap()
                .font_size,
            Some(10.0)
        );
    }

    #[test]
    fn cyclic_field_kids_are_skipped_with_a_warning() {
        let mut s = Synth::new();
        let field = s.add(dictionary! {
            "T" => Object::string_literal("cycle"),
        });
        s.doc
            .get_object_mut(field)
            .unwrap()
            .as_dict_mut()
            .unwrap()
            .set("Kids", vec![Object::Reference(field)]);
        s.fields.push(field);

        let (doc, page_ids) = s.finish();
        let imported = parse(&doc, &page_ids);

        assert!(imported.fields.is_empty());
        assert!(imported
            .warnings
            .iter()
            .any(|warning| warning.contains("cycle or repeated reference")));
    }

    #[test]
    fn excessively_deep_field_kids_are_skipped_with_a_warning() {
        let mut s = Synth::new();
        let fields: Vec<ObjectId> = (0..=FIELD_DEPTH + 1)
            .map(|index| {
                s.add(dictionary! {
                    "T" => Object::string_literal(format!("level-{index}")),
                })
            })
            .collect();
        for pair in fields.windows(2) {
            s.doc
                .get_object_mut(pair[0])
                .unwrap()
                .as_dict_mut()
                .unwrap()
                .set("Kids", vec![Object::Reference(pair[1])]);
        }
        s.fields.push(fields[0]);

        let (doc, page_ids) = s.finish();
        let imported = parse(&doc, &page_ids);

        assert!(imported.fields.is_empty());
        assert!(imported
            .warnings
            .iter()
            .any(|warning| warning.contains("exceeds maximum field depth")));
    }

    #[test]
    fn signature_field_occupancy_is_fail_closed() {
        let mut doc = Document::new();
        let signed = doc.add_object(dictionary! {
            "Type" => "Sig",
            "ByteRange" => vec![Object::Integer(0), Object::Integer(10), Object::Integer(20), Object::Integer(30)],
        });
        let parent = doc.add_object(dictionary! { "V" => Object::Reference(signed) });
        let empty = dictionary! {};
        let inherited = dictionary! { "Parent" => Object::Reference(parent) };
        let inherited_through_null =
            dictionary! { "V" => Object::Null, "Parent" => Object::Reference(parent) };
        let direct = dictionary! { "V" => dictionary! { "ByteRange" => vec![Object::Integer(0)] } };
        let null = dictionary! { "V" => Object::Null };
        let malformed = dictionary! { "V" => Object::string_literal("not a signature") };
        let dangling = dictionary! { "V" => Object::Reference((9999, 0)) };
        let missing_parent = dictionary! { "Parent" => Object::Reference((9998, 0)) };

        assert_eq!(
            signature_field_state(&doc, &empty),
            SignatureFieldState::Empty
        );
        assert_eq!(
            signature_field_state(&doc, &null),
            SignatureFieldState::Empty
        );
        assert_eq!(
            signature_field_state(&doc, &inherited),
            SignatureFieldState::Occupied
        );
        assert_eq!(
            signature_field_state(&doc, &inherited_through_null),
            SignatureFieldState::Occupied
        );
        assert_eq!(
            signature_field_state(&doc, &direct),
            SignatureFieldState::Occupied
        );
        for field in [&malformed, &dangling, &missing_parent] {
            assert_eq!(
                signature_field_state(&doc, field),
                SignatureFieldState::Unknown
            );
        }
    }

    #[test]
    fn signature_fixture_is_empty_and_has_one_widget() {
        let doc = Document::load(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/sig-field.pdf"),
        )
        .unwrap();
        let imp = parse(&doc, &page_ids(&doc));
        assert_eq!(imp.fields.len(), 1);
        let field = &imp.fields[0];
        assert_eq!(field.name, "Signature1");
        assert_eq!(field.field_type, FieldType::Signature);
        assert_eq!(field.signature_state, Some(SignatureFieldState::Empty));
        assert_eq!(field.widgets.len(), 1);
        assert_eq!(
            field.widgets[0].rect,
            crate::geom::Rect::new(40.0, 70.0, 260.0, 150.0)
        );
    }

    #[test]
    fn duplicate_signature_field_names_are_unknown() {
        let mut s = Synth::new();
        for x in [10, 100] {
            let widget = s.add(dictionary! {
                "Subtype" => "Widget",
                "FT" => "Sig",
                "T" => Object::string_literal("Duplicate"),
                "Rect" => vec![
                    Object::Integer(x), Object::Integer(10),
                    Object::Integer(x + 80), Object::Integer(40),
                ],
                "P" => Object::Reference(s.page),
            });
            s.fields.push(widget);
            s.annots.push(widget);
        }
        let (doc, page_ids) = s.finish();
        let imported = parse(&doc, &page_ids);
        assert_eq!(imported.fields.len(), 2);
        assert!(imported
            .fields
            .iter()
            .all(|field| field.signature_state == Some(SignatureFieldState::Unknown)));
        assert!(imported
            .warnings
            .iter()
            .any(|warning| warning.contains("duplicate")));
    }

    #[test]
    fn signature_field_flags_are_resolved_and_malformed_flags_fail_closed() {
        let mut s = Synth::new();
        let readonly = s.doc.add_object(Object::Integer(1));
        for (name, flags) in [
            ("ReadOnly", Object::Reference(readonly)),
            ("Unknown", Object::Reference((9999, 0))),
        ] {
            let widget = s.add(dictionary! {
                "Subtype" => "Widget",
                "FT" => "Sig",
                "T" => Object::string_literal(name),
                "Ff" => flags,
                "Rect" => vec![
                    Object::Integer(10), Object::Integer(10),
                    Object::Integer(90), Object::Integer(40),
                ],
                "P" => Object::Reference(s.page),
            });
            s.fields.push(widget);
            s.annots.push(widget);
        }
        let (doc, page_ids) = s.finish();
        let imported = parse(&doc, &page_ids);
        let readonly = imported
            .fields
            .iter()
            .find(|field| field.name == "ReadOnly")
            .unwrap();
        assert!(readonly.flags.read_only);
        assert_eq!(readonly.signature_state, Some(SignatureFieldState::Empty));
        let unknown = imported
            .fields
            .iter()
            .find(|field| field.name == "Unknown")
            .unwrap();
        assert_eq!(unknown.signature_state, Some(SignatureFieldState::Unknown));
    }

    #[test]
    fn hierarchy_resolution() {
        let mut s = Synth::new();
        let gp = s.doc.new_object_id();
        let parent = s.doc.new_object_id();
        let rect = [10.0, 10.0, 100.0, 30.0];
        let leaf = s.add(dictionary! {
            "Type" => "Annot",
            "Subtype" => "Widget",
            "Parent" => Object::Reference(parent),
            "T" => Object::string_literal("c"),
            "Rect" => vec![
                Object::from(rect[0]), Object::from(rect[1]),
                Object::from(rect[2]), Object::from(rect[3]),
            ],
            "P" => Object::Reference(s.page),
        });
        s.doc.objects.insert(
            parent,
            Object::Dictionary(dictionary! {
                "Parent" => Object::Reference(gp),
                "T" => Object::string_literal("b"),
                "Ff" => 1, // readOnly
                "Kids" => vec![Object::Reference(leaf)],
            }),
        );
        s.doc.objects.insert(
            gp,
            Object::Dictionary(dictionary! {
                "T" => Object::string_literal("a"),
                "FT" => "Tx",
                "DA" => Object::string_literal("/TiRo 11 Tf 0 g"),
                "Kids" => vec![Object::Reference(parent)],
            }),
        );
        s.annots.push(leaf);
        s.fields.push(gp);
        let (doc, page_ids) = s.finish();
        let imp = parse(&doc, &page_ids);
        assert_eq!(imp.fields.len(), 1, "{:?}", imp.warnings);
        let f = &imp.fields[0];
        assert_eq!(f.name, "a.b.c");
        assert_eq!(f.field_type, FieldType::Text);
        assert!(f.flags.read_only, "flags inherited through parent");
        assert_eq!(
            f.default_appearance
                .as_ref()
                .unwrap()
                .font_family
                .as_deref(),
            Some("Times-Roman")
        );
        assert_eq!(f.widgets.len(), 1);
        assert_eq!(f.widgets[0].page, 0);
        assert!(imp.refs["a.b.c"].kids_are_merged);
    }

    #[test]
    fn merged_dict_widget() {
        let doc = fixture();
        let imp = parse(&doc, &page_ids(&doc));
        let r = &imp.refs["name"];
        assert!(r.kids_are_merged);
        assert_eq!(r.field, r.widgets[0].0);
        let r = &imp.refs["bottom.note"];
        assert!(r.kids_are_merged);
        let r = &imp.refs["color"];
        assert!(!r.kids_are_merged);
        assert_eq!(r.widgets.len(), 3);
    }

    #[test]
    fn radio_unison_two_kids_same_export() {
        let mut s = Synth::new();
        let parent = s.doc.new_object_id();
        let mut kids = Vec::new();
        for i in 0..2 {
            let rect = [10.0 + i as f64 * 30.0, 10.0, 30.0 + i as f64 * 30.0, 30.0];
            let ap = ap_n(&mut s.doc, rect, "Yes");
            let kid = s.add(dictionary! {
                "Type" => "Annot",
                "Subtype" => "Widget",
                "Parent" => Object::Reference(parent),
                "Rect" => vec![
                    Object::from(rect[0]), Object::from(rect[1]),
                    Object::from(rect[2]), Object::from(rect[3]),
                ],
                "P" => Object::Reference(s.page),
                "AP" => ap,
                "AS" => "Off",
            });
            kids.push(Object::Reference(kid));
            s.annots.push(kid);
        }
        s.doc.objects.insert(
            parent,
            Object::Dictionary(dictionary! {
                "FT" => "Btn",
                "Ff" => (1 << 15) | (1 << 25), // radio + radiosInUnison
                "T" => Object::string_literal("g"),
                "V" => Object::Name(b"Yes".to_vec()),
                "Kids" => kids,
            }),
        );
        s.fields.push(parent);
        let (doc, page_ids) = s.finish();
        let imp = parse(&doc, &page_ids);
        let f = &imp.fields[0];
        assert_eq!(f.name, "g");
        assert_eq!(f.field_type, FieldType::Radio);
        assert!(f.flags.radios_in_unison);
        assert_eq!(f.export_values, Some(vec!["Yes".to_string()]));
        assert_eq!(f.widgets.len(), 2);
        for w in &f.widgets {
            assert_eq!(w.export_value.as_deref(), Some("Yes"));
        }
        assert_eq!(f.value, Some(Value::String("Yes".into())));
    }
}
