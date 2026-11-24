use std::collections::HashSet;

use const_oid::{db::rfc5912, AssociatedOid};
use der::{Decode, Encode};
use lopdf::{Document, Object, ObjectId};
use nib_core::forms;
use nib_core::model::{DigestAlgorithm, FieldType, SigLevel, SignatureAlgorithm, SubFilter};
use sha2::{Digest, Sha256};
use x509_cert::ext::pkix::{BasicConstraints, KeyUsage};
use x509_cert::Certificate;

use crate::{algorithm, cms, Scheme, SignError};

const MAX_CMS_BYTES: usize = 1 << 20;
const MAX_PATH_STEPS: usize = 256;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IntegrityStatus {
    Valid,
    Invalid,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrustStatus {
    NotEvaluated,
    Trusted,
    Untrusted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ModificationKind {
    FormFill,
    FormStructure,
    Annotation,
    Content,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignatureVerification {
    pub field_name: String,
    pub integrity: IntegrityStatus,
    pub trust: TrustStatus,
    pub trust_errors: Vec<String>,
    pub revocation_checked: bool,
    pub ltv_checked: bool,
    pub claimed_signing_time: Option<String>,
    pub revision: Option<u32>,
    pub revision_end: Option<u64>,
    pub covers_whole_document: bool,
    pub doc_mdp: Option<u8>,
    pub modifications_after: Vec<ModificationKind>,
    pub doc_mdp_violated: Option<bool>,
    pub locked_fields: Vec<String>,
    pub field_lock_violated: Option<bool>,
    pub errors: Vec<String>,
}

impl SignatureVerification {
    fn invalid(field_name: String, error: impl Into<String>) -> Self {
        Self {
            field_name,
            integrity: IntegrityStatus::Invalid,
            trust: TrustStatus::NotEvaluated,
            trust_errors: Vec::new(),
            revocation_checked: false,
            ltv_checked: false,
            claimed_signing_time: None,
            revision: None,
            revision_end: None,
            covers_whole_document: false,
            doc_mdp: None,
            modifications_after: Vec::new(),
            doc_mdp_violated: None,
            locked_fields: Vec::new(),
            field_lock_violated: None,
            errors: vec![error.into()],
        }
    }
}

/// Verify every occupied signature field in a PDF byte stream. Trust is
/// deliberately not evaluated without caller-supplied anchors.
pub fn verify_signatures(
    bytes: &[u8],
    trust_anchors: &[Vec<u8>],
) -> Result<Vec<SignatureVerification>, SignError> {
    let doc = Document::load_mem(bytes).map_err(|e| SignError::InvalidCms(format!("PDF: {e}")))?;
    let page_ids: Vec<ObjectId> = doc.get_pages().values().copied().collect();
    let imported = forms::parse(&doc, &page_ids);
    let mut results = Vec::new();
    let mut signature_objects = HashSet::new();
    let mut gaps = Vec::<(usize, usize, String)>::new();

    for field in imported
        .fields
        .iter()
        .filter(|f| f.field_type == FieldType::Signature)
    {
        let Some(refs) = imported.refs.get(&field.name) else {
            continue;
        };
        let field_dict = match doc.get_object(refs.field).and_then(Object::as_dict) {
            Ok(d) => d,
            Err(_) => {
                results.push(SignatureVerification::invalid(
                    field.name.clone(),
                    "signature field dictionary is malformed",
                ));
                continue;
            }
        };
        let value = match field_dict.get(b"V") {
            Ok(Object::Null) | Err(_) => continue,
            Ok(v) => resolve(&doc, v),
        };
        let sig_dict = match value {
            Ok(Object::Dictionary(d)) if d.has(b"ByteRange") || d.has(b"Contents") => d,
            Ok(_) => continue,
            Err(e) => {
                results.push(SignatureVerification::invalid(field.name.clone(), e));
                continue;
            }
        };
        let object_id = field_dict
            .get(b"V")
            .ok()
            .and_then(|v| v.as_reference().ok())
            .unwrap_or(refs.field);
        if !signature_objects.insert(object_id) {
            results.push(SignatureVerification::invalid(
                field.name.clone(),
                "signature dictionary is shared by multiple fields (shadowing)",
            ));
            continue;
        }

        match verify_one(
            bytes,
            field.name.clone(),
            object_id,
            refs.field,
            sig_dict,
            trust_anchors,
        ) {
            Ok((result, gap)) => {
                gaps.push((gap.0, gap.1, field.name.clone()));
                results.push(result);
            }
            Err(error) => results.push(SignatureVerification::invalid(field.name.clone(), error)),
        }
    }

    gaps.sort_by_key(|g| g.0);
    for pair in gaps.windows(2) {
        if pair[0].1 > pair[1].0 {
            for name in [&pair[0].2, &pair[1].2] {
                if let Some(result) = results.iter_mut().find(|r| &r.field_name == name) {
                    result.integrity = IntegrityStatus::Invalid;
                    result
                        .errors
                        .push("signature /Contents ranges overlap or are shadowed".into());
                }
            }
        }
    }
    Ok(results)
}

fn resolve<'a>(doc: &'a Document, object: &'a Object) -> Result<&'a Object, String> {
    match object {
        Object::Reference(id) => doc
            .get_object(*id)
            .map_err(|e| format!("signature value reference is malformed: {e}")),
        other => Ok(other),
    }
}

fn verify_one(
    bytes: &[u8],
    field_name: String,
    object_id: ObjectId,
    field_object_id: ObjectId,
    sig: &lopdf::Dictionary,
    trust_anchors: &[Vec<u8>],
) -> Result<(SignatureVerification, (usize, usize)), String> {
    let br = sig
        .get(b"ByteRange")
        .and_then(Object::as_array)
        .map_err(|e| format!("malformed /ByteRange: {e}"))?;
    let [a, b, c, d] = br.as_slice() else {
        return Err("/ByteRange must contain exactly four integers".into());
    };
    let range = [as_offset(a)?, as_offset(b)?, as_offset(c)?, as_offset(d)?];
    let [start, first_len, second_start, second_len] = range;
    let gap_end = second_start;
    let signed_end = second_start
        .checked_add(second_len)
        .ok_or_else(|| "/ByteRange end overflows".to_string())?;
    let first_end = start
        .checked_add(first_len)
        .ok_or_else(|| "/ByteRange first range overflows".to_string())?;
    if start != 0
        || first_len == 0
        || second_start <= first_end
        || signed_end > bytes.len() as u64
        || first_end > bytes.len() as u64
    {
        return Err(format!("invalid /ByteRange {range:?}"));
    }
    if !is_revision_boundary(bytes, signed_end as usize) {
        return Err("/ByteRange does not end at a complete PDF revision".into());
    }
    let gap = (
        usize::try_from(first_end).map_err(|_| "/ByteRange offset overflow")?,
        usize::try_from(gap_end).map_err(|_| "/ByteRange offset overflow")?,
    );
    if gap.1 <= gap.0
        || bytes.get(gap.0) != Some(&b'<')
        || bytes.get(gap.1.wrapping_sub(1)) != Some(&b'>')
    {
        return Err("/ByteRange gap is not exactly a /Contents hex string".into());
    }

    let contents = sig
        .get(b"Contents")
        .and_then(Object::as_str)
        .map_err(|e| format!("malformed /Contents: {e}"))?;
    if contents.len() > MAX_CMS_BYTES {
        return Err(format!("/Contents exceeds {MAX_CMS_BYTES} bytes"));
    }
    let cms_len = cms::der_prefix_len(contents).map_err(|e| e.to_string())?;
    if cms_len > MAX_CMS_BYTES {
        return Err(format!("CMS exceeds {MAX_CMS_BYTES} bytes"));
    }
    if contents[cms_len..].iter().any(|&b| b != 0) {
        return Err("/Contents has non-zero data after the CMS object".into());
    }
    let cms_der = &contents[..cms_len];
    verify_raw_contents(bytes, object_id, range, gap, contents)
        .map_err(|e| format!("invalid signature object range: {e}"))?;

    let signed_doc = Document::load_mem(&bytes[..signed_end as usize])
        .map_err(|e| format!("signed revision PDF is malformed: {e}"))?;
    let signed_field = signed_doc
        .get_object(field_object_id)
        .and_then(Object::as_dict)
        .map_err(|e| format!("signed signature field is malformed: {e}"))?;
    let signed_value = signed_field
        .get(b"V")
        .map_err(|e| format!("signed signature field has no /V: {e}"))?;
    let signed_object_id = signed_value.as_reference().ok().unwrap_or(field_object_id);
    if signed_object_id != object_id {
        return Err("signature field /V changed after its signed revision".into());
    }
    let signed_sig = resolve(&signed_doc, signed_value)
        .map_err(|e| format!("signed signature dictionary is malformed: {e}"))?
        .as_dict()
        .map_err(|e| format!("signed signature dictionary is malformed: {e}"))?;
    let signed_range = signed_sig
        .get(b"ByteRange")
        .and_then(Object::as_array)
        .map_err(|e| format!("signed /ByteRange is malformed: {e}"))?;
    let signed_range = signed_range
        .iter()
        .map(as_offset)
        .collect::<Result<Vec<_>, _>>()?;
    if signed_range.as_slice() != range {
        return Err("signature /ByteRange changed after its signed revision".into());
    }
    let signed_contents = signed_sig
        .get(b"Contents")
        .and_then(Object::as_str)
        .map_err(|e| format!("signed /Contents is malformed: {e}"))?;
    if signed_contents != contents {
        return Err("signature /Contents changed after its signed revision".into());
    }

    let sub_filter = match signed_sig.get(b"SubFilter").and_then(Object::as_name) {
        Ok(b"ETSI.CAdES.detached") => SubFilter::EtsiCadesDetached,
        Ok(b"adbe.pkcs7.detached") => SubFilter::AdbePkcs7Detached,
        Ok(name) => {
            return Err(format!(
                "unsupported signature /SubFilter /{}",
                String::from_utf8_lossy(name)
            ))
        }
        Err(e) => return Err(format!("missing or malformed /SubFilter: {e}")),
    };
    let mut pdf_digest = Vec::new();
    pdf_digest.extend_from_slice(&bytes[..gap.0]);
    pdf_digest.extend_from_slice(&bytes[gap.1..signed_end as usize]);
    let (leaf, certificates, _scheme) =
        cms::verify_existing(cms_der, &pdf_digest, sub_filter).map_err(|e| e.to_string())?;

    let (trust, trust_errors) = if trust_anchors.is_empty() {
        (TrustStatus::NotEvaluated, Vec::new())
    } else {
        trust_status(&leaf, &certificates, trust_anchors)
    };
    let mut result = SignatureVerification {
        field_name: field_name.clone(),
        integrity: IntegrityStatus::Valid,
        trust,
        trust_errors,
        revocation_checked: false,
        ltv_checked: false,
        claimed_signing_time: signed_sig.get(b"M").ok().and_then(|value| match value {
            Object::String(..) => lopdf::decode_text_string(value).ok(),
            Object::Name(bytes) => Some(String::from_utf8_lossy(bytes).into_owned()),
            _ => None,
        }),
        revision: Some(revision_number(bytes, signed_end as usize)),
        revision_end: Some(signed_end),
        covers_whole_document: signed_end == bytes.len() as u64,
        doc_mdp: doc_mdp(&signed_doc, signed_sig),
        modifications_after: Vec::new(),
        doc_mdp_violated: None,
        locked_fields: locked_fields(&signed_doc, signed_field, signed_sig),
        field_lock_violated: None,
        errors: Vec::new(),
    };

    if signed_end < bytes.len() as u64 {
        result.modifications_after = classify_changes(bytes, signed_end as usize);
        if let Some(permission) = result.doc_mdp {
            result.doc_mdp_violated = Some(
                result
                    .modifications_after
                    .iter()
                    .any(|change| !permitted(permission, *change)),
            );
        }
        if !result.locked_fields.is_empty() {
            result.field_lock_violated = Some(match changed_fields(bytes, signed_end as usize) {
                Some(changed) => result
                    .locked_fields
                    .iter()
                    .any(|name| changed.contains(name)),
                None => true,
            });
        }
    }
    Ok((result, gap))
}

fn as_offset(object: &Object) -> Result<u64, String> {
    object
        .as_i64()
        .map_err(|e| format!("ByteRange value is not an integer: {e}"))
        .and_then(|n| u64::try_from(n).map_err(|_| "negative /ByteRange value".into()))
}

fn verify_raw_contents(
    bytes: &[u8],
    object_id: ObjectId,
    range: [u64; 4],
    gap: (usize, usize),
    contents: &[u8],
) -> Result<(), String> {
    let header = format!("{} {} obj", object_id.0, object_id.1);
    let revision_end = range[2]
        .checked_add(range[3])
        .ok_or_else(|| "revision end overflow".to_string())?;
    let end = usize::try_from(revision_end).map_err(|_| "revision end overflow")?;
    let mut matching = 0;
    for start in find_all(&bytes[..end], header.as_bytes()) {
        if (start > 0 && !is_ws(bytes[start - 1]))
            || bytes
                .get(start + header.len())
                .is_some_and(|b| !is_delim(*b))
        {
            continue;
        }
        let body_start = start + header.len();
        let Some(endobj) = find_all(&bytes[body_start..end], b"endobj")
            .into_iter()
            .find(|&i| i > 0 && is_ws(bytes[body_start + i - 1]))
        else {
            continue;
        };
        let span = &bytes[body_start..body_start + endobj];
        if !span
            .iter()
            .rposition(|b| !is_ws(*b))
            .is_some_and(|i| span[..=i].ends_with(b">>"))
        {
            continue;
        }
        let br_keys = find_key(span, b"/ByteRange");
        let contents_keys = find_key(span, b"/Contents");
        if br_keys.len() != 1 || contents_keys.len() != 1 {
            continue;
        }
        let raw_br = array_after(span, br_keys[0] + b"/ByteRange".len())?;
        if parse_range(raw_br)? != range {
            continue;
        }
        let value_offset = skip_ws(span, contents_keys[0] + b"/Contents".len());
        let absolute = body_start + value_offset;
        if absolute != gap.0 || gap.1 > end {
            continue;
        }
        let raw = &bytes[gap.0 + 1..gap.1 - 1];
        if decode_hex(raw)? != contents {
            continue;
        }
        matching += 1;
    }
    if matching != 1 {
        return Err(format!(
            "expected one exact /ByteRange and /Contents token pair, found {matching}"
        ));
    }
    Ok(())
}

fn parse_range(bytes: &[u8]) -> Result<[u64; 4], String> {
    let values = bytes
        .split(|b| is_ws(*b))
        .filter(|s| !s.is_empty())
        .map(|s| {
            std::str::from_utf8(s)
                .ok()
                .and_then(|s| s.parse::<u64>().ok())
                .ok_or_else(|| "malformed raw /ByteRange".to_string())
        })
        .collect::<Result<Vec<_>, _>>()?;
    values
        .try_into()
        .map_err(|_| "raw /ByteRange must have four values".into())
}

fn array_after(bytes: &[u8], start: usize) -> Result<&[u8], String> {
    let open = skip_ws(bytes, start);
    if bytes.get(open) != Some(&b'[') {
        return Err("raw /ByteRange is not an array".into());
    }
    let close = bytes[open + 1..]
        .iter()
        .position(|b| *b == b']')
        .map(|i| open + 1 + i)
        .ok_or_else(|| "raw /ByteRange is unterminated".to_string())?;
    Ok(&bytes[open + 1..close])
}

fn decode_hex(raw: &[u8]) -> Result<Vec<u8>, String> {
    let digits: Vec<_> = raw.iter().copied().filter(|b| !is_ws(*b)).collect();
    if !digits.len().is_multiple_of(2) {
        return Err("odd number of /Contents hex digits".into());
    }
    digits
        .as_chunks::<2>()
        .0
        .iter()
        .map(|[high_digit, low_digit]| {
            let high = hex_digit(*high_digit)?;
            let low = hex_digit(*low_digit)?;
            Ok((high << 4) | low)
        })
        .collect()
}

fn hex_digit(b: u8) -> Result<u8, String> {
    match b {
        b'0'..=b'9' => Ok(b - b'0'),
        b'a'..=b'f' => Ok(b - b'a' + 10),
        b'A'..=b'F' => Ok(b - b'A' + 10),
        _ => Err("non-hex character in /Contents".into()),
    }
}

fn find_all(bytes: &[u8], needle: &[u8]) -> Vec<usize> {
    if needle.is_empty() || bytes.len() < needle.len() {
        return Vec::new();
    }
    bytes
        .windows(needle.len())
        .enumerate()
        .filter_map(|(i, window)| (window == needle).then_some(i))
        .collect()
}

fn is_ws(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\r' | b'\n' | b'\x0c' | b'\0')
}

fn is_delim(b: u8) -> bool {
    is_ws(b)
        || matches!(
            b,
            b'(' | b')' | b'<' | b'>' | b'[' | b']' | b'{' | b'}' | b'/' | b'%'
        )
}

fn skip_ws(bytes: &[u8], mut index: usize) -> usize {
    while bytes.get(index).is_some_and(|b| is_ws(*b)) {
        index += 1;
    }
    index
}

fn find_key(bytes: &[u8], key: &[u8]) -> Vec<usize> {
    find_all(bytes, key)
        .into_iter()
        .filter(|&i| bytes.get(i + key.len()).is_none_or(|b| is_delim(*b)))
        .collect()
}

fn revision_number(bytes: &[u8], end: usize) -> u32 {
    find_all(&bytes[..end], b"%%EOF")
        .into_iter()
        .filter(|&start| {
            (start == 0 || matches!(bytes[start - 1], b'\r' | b'\n'))
                && (start + 5 == end || bytes.get(start + 5).is_some_and(|b| is_ws(*b)))
        })
        .count()
        .min(u32::MAX as usize) as u32
}

fn is_revision_boundary(bytes: &[u8], end: usize) -> bool {
    let Some(prefix) = bytes.get(..end) else {
        return false;
    };
    let Some(start) = find_all(prefix, b"%%EOF").into_iter().rev().find(|&start| {
        (start == 0 || matches!(prefix[start - 1], b'\r' | b'\n'))
            && prefix[start + 5..].iter().all(|b| is_ws(*b))
    }) else {
        return false;
    };
    let _ = start;
    Document::load_mem(prefix).is_ok()
}

fn classify_changes(bytes: &[u8], signed_end: usize) -> Vec<ModificationKind> {
    let Ok(base) = Document::load_mem(&bytes[..signed_end]) else {
        return vec![ModificationKind::Unknown];
    };
    let Ok(current) = Document::load_mem(bytes) else {
        return vec![ModificationKind::Unknown];
    };
    let mut changes = HashSet::new();
    if form_field_names(&base) != form_field_names(&current) {
        changes.insert(ModificationKind::FormStructure);
    }
    let mut appearance_roles = appearance_object_roles(&base);
    for (id, role) in appearance_object_roles(&current) {
        appearance_roles.entry(id).or_insert(role);
    }
    let ids: HashSet<_> = base
        .objects
        .keys()
        .chain(current.objects.keys())
        .copied()
        .collect();
    for id in ids {
        if base.objects.get(&id) == current.objects.get(&id) {
            continue;
        }
        let change = appearance_roles.get(&id).copied().unwrap_or_else(|| {
            classify_object_change(
                base.objects.get(&id),
                current.objects.get(&id),
                &base,
                &current,
            )
        });
        changes.insert(change);
    }
    if changes.is_empty() {
        changes.insert(ModificationKind::Unknown);
    }
    let mut out: Vec<_> = changes.into_iter().collect();
    out.sort_by_key(|change| match change {
        ModificationKind::FormFill => 0,
        ModificationKind::FormStructure => 1,
        ModificationKind::Annotation => 2,
        ModificationKind::Content => 3,
        ModificationKind::Unknown => 4,
    });
    out
}

fn classify_object_change(
    old: Option<&Object>,
    new: Option<&Object>,
    base: &Document,
    current: &Document,
) -> ModificationKind {
    let Some(object) = new.or(old) else {
        return ModificationKind::Unknown;
    };
    if let Object::Array(items) = object {
        if !items.is_empty() {
            let mut categories = HashSet::new();
            for item in items {
                let Ok(id) = item.as_reference() else {
                    return ModificationKind::Content;
                };
                let Some(candidate) = current.objects.get(&id).or_else(|| base.objects.get(&id))
                else {
                    return ModificationKind::Unknown;
                };
                categories.insert(classify_referenced_object(candidate));
            }
            if categories.len() == 1 {
                if let Some(category) = categories.into_iter().next() {
                    return category;
                }
            }
        }
        return ModificationKind::Content;
    }
    let Object::Dictionary(dict) = object else {
        return ModificationKind::Content;
    };
    if dict.get(b"Type").and_then(Object::as_name).ok() == Some(b"XRef") {
        return ModificationKind::Unknown;
    }
    if dict.get(b"Subtype").and_then(Object::as_name).ok() == Some(b"Widget")
        || dict.get(b"FT").and_then(Object::as_name).ok() == Some(b"Sig")
        || dict.get(b"Type").and_then(Object::as_name).ok() == Some(b"Sig")
        || dict.has(b"AcroForm")
        || dict.has(b"Fields")
        || dict.has(b"T")
        || dict.has(b"Perms")
    {
        return ModificationKind::FormFill;
    }
    if dict.get(b"Type").and_then(Object::as_name).ok() == Some(b"Annot")
        || dict.has(b"Rect") && dict.has(b"Subtype")
    {
        return ModificationKind::Annotation;
    }
    if dict.get(b"Type").and_then(Object::as_name).ok() == Some(b"Page") {
        let (Some(Object::Dictionary(old)), Some(Object::Dictionary(new))) = (old, new) else {
            return ModificationKind::Content;
        };
        let mut old = old.clone();
        let mut new = new.clone();
        old.remove(b"Annots");
        new.remove(b"Annots");
        return if old == new {
            ModificationKind::Annotation
        } else {
            ModificationKind::Content
        };
    }
    ModificationKind::Content
}

fn classify_referenced_object(object: &Object) -> ModificationKind {
    let Object::Dictionary(dict) = object else {
        return ModificationKind::Content;
    };
    if dict.get(b"Subtype").and_then(Object::as_name).ok() == Some(b"Widget")
        || dict.get(b"FT").and_then(Object::as_name).ok() == Some(b"Sig")
        || dict.get(b"Type").and_then(Object::as_name).ok() == Some(b"Sig")
        || dict.has(b"T")
    {
        ModificationKind::FormFill
    } else if dict.get(b"Type").and_then(Object::as_name).ok() == Some(b"Page") {
        ModificationKind::Content
    } else if dict.get(b"Type").and_then(Object::as_name).ok() == Some(b"Annot")
        || dict.has(b"Subtype") && dict.has(b"Rect")
    {
        ModificationKind::Annotation
    } else {
        ModificationKind::Content
    }
}

fn appearance_object_roles(
    doc: &Document,
) -> std::collections::HashMap<ObjectId, ModificationKind> {
    let mut out = std::collections::HashMap::new();
    for object in doc.objects.values() {
        let Ok(dict) = object.as_dict() else {
            continue;
        };
        let Some(appearance) = dict.get(b"AP").ok() else {
            continue;
        };
        let role = if dict.get(b"Subtype").and_then(Object::as_name).ok() == Some(b"Widget") {
            ModificationKind::FormFill
        } else {
            ModificationKind::Annotation
        };
        let mut visited = HashSet::new();
        collect_appearance_refs(doc, appearance, role, &mut visited, &mut out, 0);
    }
    out
}

fn collect_appearance_refs(
    doc: &Document,
    object: &Object,
    role: ModificationKind,
    visited: &mut HashSet<ObjectId>,
    output: &mut std::collections::HashMap<ObjectId, ModificationKind>,
    depth: usize,
) {
    if depth >= 16 {
        return;
    }
    match object {
        Object::Reference(id) if visited.insert(*id) => {
            output.entry(*id).or_insert(role);
            if let Ok(referenced) = doc.get_object(*id) {
                collect_appearance_refs(doc, referenced, role, visited, output, depth + 1);
            }
        }
        Object::Dictionary(dict) => {
            for value in dict.iter().map(|(_, value)| value) {
                collect_appearance_refs(doc, value, role, visited, output, depth + 1);
            }
        }
        Object::Array(items) => {
            for value in items {
                collect_appearance_refs(doc, value, role, visited, output, depth + 1);
            }
        }
        Object::Stream(stream) => {
            for (_, value) in stream.dict.iter() {
                collect_appearance_refs(doc, value, role, visited, output, depth + 1);
            }
        }
        _ => {}
    }
}

fn changed_fields(bytes: &[u8], signed_end: usize) -> Option<HashSet<String>> {
    let base = Document::load_mem(&bytes[..signed_end]).ok()?;
    let current = Document::load_mem(bytes).ok()?;
    let pages = current.get_pages().values().copied().collect::<Vec<_>>();
    let before = forms::parse(
        &base,
        &base.get_pages().values().copied().collect::<Vec<_>>(),
    );
    let after = forms::parse(&current, &pages);
    let before: std::collections::HashMap<_, _> = before
        .fields
        .into_iter()
        .map(|field| (field.name.clone(), field))
        .collect();
    let after: std::collections::HashMap<_, _> = after
        .fields
        .into_iter()
        .map(|field| (field.name.clone(), field))
        .collect();
    Some(
        before
            .keys()
            .chain(after.keys())
            .filter(|name| before.get(*name) != after.get(*name))
            .cloned()
            .collect(),
    )
}

fn form_field_names(doc: &Document) -> HashSet<String> {
    let fields = forms::parse(doc, &doc.get_pages().values().copied().collect::<Vec<_>>());
    fields.fields.into_iter().map(|field| field.name).collect()
}

fn permitted(permission: u8, change: ModificationKind) -> bool {
    match permission {
        1 => false,
        2 => change == ModificationKind::FormFill,
        3 => matches!(
            change,
            ModificationKind::FormFill | ModificationKind::Annotation
        ),
        _ => false,
    }
}

fn doc_mdp(doc: &Document, sig: &lopdf::Dictionary) -> Option<u8> {
    let references = resolve(doc, sig.get(b"Reference").ok()?)
        .ok()?
        .as_array()
        .ok()?;
    references.iter().find_map(|reference| {
        let dictionary = resolve(doc, reference).ok()?.as_dict().ok()?;
        if dictionary
            .get(b"TransformMethod")
            .and_then(Object::as_name)
            .ok()
            != Some(b"DocMDP")
        {
            return None;
        }
        resolve(doc, dictionary.get(b"TransformParams").ok()?)
            .ok()?
            .as_dict()
            .ok()?
            .get(b"P")
            .ok()?
            .as_i64()
            .ok()
            .and_then(|p| u8::try_from(p).ok())
            .filter(|p| (1..=3).contains(p))
    })
}

fn locked_fields(
    doc: &Document,
    field: &lopdf::Dictionary,
    sig: &lopdf::Dictionary,
) -> Vec<String> {
    let mut fields = HashSet::new();
    if let Some(lock) = field
        .get(b"Lock")
        .ok()
        .and_then(|o| resolve(doc, o).ok())
        .and_then(|o| o.as_dict().ok())
    {
        collect_lock_names(doc, lock, &mut fields);
    }
    if let Some(lock) = sig
        .get(b"Lock")
        .ok()
        .and_then(|o| resolve(doc, o).ok())
        .and_then(|o| o.as_dict().ok())
    {
        collect_lock_names(doc, lock, &mut fields);
    }
    if let Some(references) = sig
        .get(b"Reference")
        .ok()
        .and_then(|o| resolve(doc, o).ok())
        .and_then(|o| o.as_array().ok())
    {
        for reference in references {
            let Ok(reference) = resolve(doc, reference) else {
                continue;
            };
            let Ok(reference) = reference.as_dict() else {
                continue;
            };
            if reference
                .get(b"TransformMethod")
                .and_then(Object::as_name)
                .ok()
                != Some(b"FieldMDP")
            {
                continue;
            }
            if let Some(params) = reference
                .get(b"TransformParams")
                .ok()
                .and_then(|o| resolve(doc, o).ok())
                .and_then(|o| o.as_dict().ok())
            {
                collect_lock_names(doc, params, &mut fields);
            }
        }
    }
    let mut fields: Vec<_> = fields.into_iter().collect();
    fields.sort();
    fields
}

fn collect_lock_names(doc: &Document, lock: &lopdf::Dictionary, fields: &mut HashSet<String>) {
    let field_names = lock
        .get(b"Fields")
        .ok()
        .and_then(|o| resolve(doc, o).ok())
        .and_then(|o| o.as_array().ok());
    let parsed_names = field_names.and_then(|array| {
        array
            .iter()
            .map(|value| match value {
                Object::String(..) => lopdf::decode_text_string(value).ok(),
                Object::Name(bytes) => Some(String::from_utf8_lossy(bytes).into_owned()),
                _ => None,
            })
            .collect::<Option<Vec<_>>>()
    });
    let action = match lock.get(b"Action").and_then(Object::as_name) {
        Ok(b"Include") if parsed_names.is_some() => b"Include".as_slice(),
        Ok(b"Exclude") if parsed_names.is_some() => b"Exclude".as_slice(),
        _ => b"All".as_slice(),
    };
    let names = parsed_names.unwrap_or_default();
    let all_names = forms::parse(doc, &doc.get_pages().values().copied().collect::<Vec<_>>())
        .fields
        .into_iter()
        .map(|field| field.name)
        .collect::<Vec<_>>();
    for name in all_names {
        let listed = names.iter().any(|n| n == &name);
        let locked = match action {
            b"Include" => listed,
            b"Exclude" => !listed,
            _ => true,
        };
        if locked {
            fields.insert(name);
        }
    }
}

fn trust_status(
    leaf: &Certificate,
    certificates: &[Certificate],
    anchors: &[Vec<u8>],
) -> (TrustStatus, Vec<String>) {
    if anchors.len() > 64 {
        return (
            TrustStatus::Untrusted,
            vec!["more than 64 trust anchors supplied".into()],
        );
    }
    if !certificate_valid_now(leaf) {
        return (
            TrustStatus::Untrusted,
            vec!["signing certificate is outside its validity period at verification time".into()],
        );
    }
    if !can_sign_pdf(leaf) {
        return (
            TrustStatus::Untrusted,
            vec!["signing certificate key usage does not permit digital signatures".into()],
        );
    }
    let mut candidates = certificates.to_vec();
    let mut anchor_certs = Vec::new();
    for anchor in anchors {
        if anchor.len() > MAX_CMS_BYTES {
            continue;
        }
        if let Ok(cert) = Certificate::from_der(anchor) {
            anchor_certs.push(cert);
        }
    }
    candidates.extend(anchor_certs.iter().cloned());
    if candidates.len() > 64 {
        return (
            TrustStatus::Untrusted,
            vec!["certificate path exceeds the 64-certificate validation limit".into()],
        );
    }
    let mut remaining = MAX_PATH_STEPS;
    if build_trusted_path(leaf, &candidates, &anchor_certs, &mut remaining) {
        (TrustStatus::Trusted, Vec::new())
    } else {
        (
            TrustStatus::Untrusted,
            vec!["certificate path did not validate to a supplied trust anchor".into()],
        )
    }
}

fn certificate_fingerprint(certificate: &Certificate) -> Option<[u8; 32]> {
    certificate
        .to_der()
        .ok()
        .map(|der| Sha256::digest(der).into())
}

fn certificate_valid_now(certificate: &Certificate) -> bool {
    let validity = certificate.tbs_certificate().validity();
    let now = std::time::SystemTime::now();
    validity.not_before.to_system_time() <= now && now <= validity.not_after.to_system_time()
}

fn build_trusted_path(
    leaf: &Certificate,
    candidates: &[Certificate],
    anchors: &[Certificate],
    remaining: &mut usize,
) -> bool {
    let mut path = Vec::new();
    let mut seen = HashSet::new();
    path_walk(
        leaf, candidates, anchors, &mut path, &mut seen, 0, remaining,
    )
}

fn path_walk(
    current: &Certificate,
    candidates: &[Certificate],
    anchors: &[Certificate],
    path: &mut Vec<Certificate>,
    seen: &mut HashSet<[u8; 32]>,
    ca_below: usize,
    remaining: &mut usize,
) -> bool {
    if *remaining == 0 {
        return false;
    }
    let Some(fingerprint) = certificate_fingerprint(current) else {
        return false;
    };
    if !seen.insert(fingerprint) {
        return false;
    }
    if !certificate_valid_now(current) || !critical_extensions_supported(current) {
        seen.remove(&fingerprint);
        return false;
    }
    if anchors.iter().any(|anchor| {
        anchor
            .to_der()
            .is_ok_and(|der| Sha256::digest(der).as_slice() == fingerprint)
    }) {
        return true;
    }
    if path.len() >= 16 {
        seen.remove(&fingerprint);
        return false;
    }
    path.push(current.clone());
    let mut trusted = false;
    for issuer in candidates {
        if *remaining == 0 {
            break;
        }
        *remaining -= 1;
        let is_anchor = certificate_fingerprint(issuer).is_some_and(|fingerprint| {
            anchors
                .iter()
                .any(|anchor| certificate_fingerprint(anchor) == Some(fingerprint))
        });
        if current.tbs_certificate().issuer() != issuer.tbs_certificate().subject()
            || !certificate_valid_now(issuer)
            || !critical_extensions_supported(issuer)
            || (!is_anchor && (!is_ca(issuer) || !can_sign_certificates(issuer)))
            || !verify_certificate_signature(current, issuer)
            || (!is_anchor && !path_length_allows(issuer, ca_below))
        {
            continue;
        }
        if path_walk(
            issuer,
            candidates,
            anchors,
            path,
            seen,
            ca_below + 1,
            remaining,
        ) {
            trusted = true;
            break;
        }
    }
    path.pop();
    seen.remove(&fingerprint);
    trusted
}

fn is_ca(certificate: &Certificate) -> bool {
    certificate
        .tbs_certificate()
        .get_extension::<BasicConstraints>()
        .ok()
        .flatten()
        .is_some_and(|(_, constraints)| constraints.ca)
}

fn can_sign_pdf(certificate: &Certificate) -> bool {
    certificate
        .tbs_certificate()
        .get_extension::<KeyUsage>()
        .ok()
        .flatten()
        .is_none_or(|(_, usage)| usage.digital_signature() || usage.non_repudiation())
}

fn can_sign_certificates(certificate: &Certificate) -> bool {
    certificate
        .tbs_certificate()
        .get_extension::<KeyUsage>()
        .ok()
        .flatten()
        .is_none_or(|(_, usage)| usage.key_cert_sign())
}

fn path_length_allows(certificate: &Certificate, ca_below: usize) -> bool {
    certificate
        .tbs_certificate()
        .get_extension::<BasicConstraints>()
        .ok()
        .flatten()
        .and_then(|(_, constraints)| constraints.path_len_constraint)
        .is_none_or(|max| ca_below <= usize::from(max))
}

fn critical_extensions_supported(certificate: &Certificate) -> bool {
    let Some(extensions) = certificate.tbs_certificate().extensions() else {
        return true;
    };
    extensions.iter().all(|extension| {
        !extension.critical
            || extension.extn_id == BasicConstraints::OID
            || extension.extn_id == KeyUsage::OID
    })
}

fn verify_certificate_signature(certificate: &Certificate, issuer: &Certificate) -> bool {
    let algorithm_id = certificate.signature_algorithm();
    if certificate.tbs_certificate().signature() != algorithm_id {
        return false;
    }
    let (digest, algorithm) = match algorithm_id.oid {
        oid if oid == rfc5912::SHA_256_WITH_RSA_ENCRYPTION => {
            (DigestAlgorithm::Sha256, SignatureAlgorithm::RsaPkcs1v15)
        }
        oid if oid == rfc5912::SHA_384_WITH_RSA_ENCRYPTION => {
            (DigestAlgorithm::Sha384, SignatureAlgorithm::RsaPkcs1v15)
        }
        oid if oid == rfc5912::SHA_512_WITH_RSA_ENCRYPTION => {
            (DigestAlgorithm::Sha512, SignatureAlgorithm::RsaPkcs1v15)
        }
        oid if oid == rfc5912::ID_RSASSA_PSS => {
            let Some(digest) = pss_digest(algorithm_id) else {
                return false;
            };
            (digest, SignatureAlgorithm::RsassaPss)
        }
        oid if oid == rfc5912::ECDSA_WITH_SHA_256 => {
            (DigestAlgorithm::Sha256, SignatureAlgorithm::EcdsaP256)
        }
        oid if oid == rfc5912::ECDSA_WITH_SHA_384 => {
            (DigestAlgorithm::Sha384, SignatureAlgorithm::EcdsaP384)
        }
        _ => return false,
    };
    let Ok(scheme) = Scheme::new(
        SubFilter::AdbePkcs7Detached,
        digest,
        algorithm,
        SigLevel::BB,
    ) else {
        return false;
    };
    if scheme.check_signature_algorithm_id(algorithm_id).is_err() {
        return false;
    }
    let Ok(tbs) = certificate.tbs_certificate().to_der() else {
        return false;
    };
    let Some(signature) = certificate.signature().as_bytes() else {
        return false;
    };
    cms::verify_signature(
        &scheme,
        issuer.tbs_certificate().subject_public_key_info(),
        &tbs,
        signature,
    )
    .is_ok()
}

fn pss_digest(algorithm_id: &spki::AlgorithmIdentifierOwned) -> Option<DigestAlgorithm> {
    let params = algorithm_id
        .parameters
        .as_ref()?
        .decode_as::<algorithm::PssParams>()
        .ok()?;
    let hash = params.hash?;
    if !algorithm::params_absent_or_null(&hash) {
        return None;
    }
    match hash.oid {
        oid if oid == rfc5912::ID_SHA_256 => Some(DigestAlgorithm::Sha256),
        oid if oid == rfc5912::ID_SHA_384 => Some(DigestAlgorithm::Sha384),
        oid if oid == rfc5912::ID_SHA_512 => Some(DigestAlgorithm::Sha512),
        _ => None,
    }
}
