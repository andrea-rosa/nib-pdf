//! Byte-level location and patching of the `/ByteRange` and `/Contents`
//! placeholders inside the staged signature revision.

use crate::SignError;

/// Width of one `/ByteRange` number slot (digits of the staging sentinel).
const SLOT_DIGITS: usize = 10;
const SENTINEL: &[u8] = b"9999999999";

/// Exact positions of the two placeholders in the staged file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Placeholder {
    /// Offset of `[` of the `/ByteRange` array.
    pub byte_range_open: usize,
    /// Offset of the matching `]`.
    pub byte_range_close: usize,
    /// Offset of `<` of the `/Contents` hex string.
    pub contents_open: usize,
    /// Offset one past `>` of the `/Contents` hex string.
    pub contents_end: usize,
}

impl Placeholder {
    /// Hex characters between the delimiters.
    pub fn hex_len(&self) -> usize {
        self.contents_end - self.contents_open - 2
    }

    /// `[0, a, b, c]`: everything except the complete `<...>` token.
    pub fn byte_range(&self, file_len: usize) -> Result<[u64; 4], SignError> {
        if self.contents_end > file_len {
            return Err(SignError::Placeholder(
                "/Contents extends past end of file".into(),
            ));
        }
        Ok([
            0,
            self.contents_open as u64,
            self.contents_end as u64,
            (file_len - self.contents_end) as u64,
        ])
    }
}

fn find_all(hay: &[u8], needle: &[u8]) -> Vec<usize> {
    if needle.is_empty() || hay.len() < needle.len() {
        return Vec::new();
    }
    (0..=hay.len() - needle.len())
        .filter(|&i| &hay[i..i + needle.len()] == needle)
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

fn skip_ws(bytes: &[u8], mut i: usize) -> usize {
    while i < bytes.len() && is_ws(bytes[i]) {
        i += 1;
    }
    i
}

/// `/Name` occurrences that are whole name tokens.
fn find_key(hay: &[u8], key: &[u8]) -> Vec<usize> {
    find_all(hay, key)
        .into_iter()
        .filter(|&i| hay.get(i + key.len()).is_none_or(|&b| is_delim(b)))
        .collect()
}

/// Locate the placeholders of signature object `obj` (`num gen obj`) inside
/// the appended revision `bytes[base_len..]`. Every lookup must be unique:
/// the object header once in the appended region, `/ByteRange` and
/// `/Contents` once inside that object and nowhere else in the appended
/// region for `/ByteRange`. The values must be the exact staging
/// placeholders (sentinel range and `expected_hex_len` zero hex digits).
pub fn locate(
    bytes: &[u8],
    base_len: usize,
    obj: (u32, u16),
    expected_hex_len: usize,
) -> Result<Placeholder, SignError> {
    if base_len >= bytes.len() {
        return Err(SignError::Placeholder("no appended revision".into()));
    }
    let tail = &bytes[base_len..];
    let header = format!("{} {} obj", obj.0, obj.1);
    let starts: Vec<usize> = find_all(tail, header.as_bytes())
        .into_iter()
        .filter(|&i| i == 0 || is_ws(tail[i - 1]))
        .filter(|&i| tail.get(i + header.len()).is_none_or(|&b| is_delim(b)))
        .collect();
    let start = match starts.as_slice() {
        [s] => *s + header.len(),
        [] => {
            return Err(SignError::Placeholder(format!(
                "signature object {header} not found in the appended revision"
            )))
        }
        _ => {
            return Err(SignError::Placeholder(format!(
                "signature object {header} is ambiguous"
            )))
        }
    };
    let end = start
        + find_all(&tail[start..], b"endobj")
            .into_iter()
            .find(|&i| is_ws(tail[start + i - 1]))
            .ok_or_else(|| SignError::Placeholder("signature object has no endobj".into()))?;
    let span = &tail[start..end];
    // The first `endobj` must close the dictionary; otherwise it sits inside
    // a string and the object boundaries are ambiguous.
    let trimmed = span.iter().rposition(|&b| !is_ws(b)).map(|p| &span[..=p]);
    if !trimmed.is_some_and(|t| t.ends_with(b">>")) {
        return Err(SignError::Placeholder(
            "signature object is not a dictionary closed before endobj".into(),
        ));
    }

    let br_keys = find_key(span, b"/ByteRange");
    let ct_keys = find_key(span, b"/Contents");
    if br_keys.len() != 1 || ct_keys.len() != 1 {
        return Err(SignError::Placeholder(format!(
            "signature object holds {} /ByteRange and {} /Contents keys; exactly one each is required",
            br_keys.len(),
            ct_keys.len()
        )));
    }
    if find_key(tail, b"/ByteRange").len() != 1 {
        return Err(SignError::Placeholder(
            "/ByteRange appears more than once in the appended revision".into(),
        ));
    }

    // /ByteRange [0 S S S]
    let mut i = skip_ws(span, br_keys[0] + b"/ByteRange".len());
    if span.get(i) != Some(&b'[') {
        return Err(SignError::Placeholder("/ByteRange is not an array".into()));
    }
    let open = i;
    let close = open
        + span[open..]
            .iter()
            .position(|&b| b == b']')
            .ok_or_else(|| SignError::Placeholder("/ByteRange array is not closed".into()))?;
    let inner = &span[open + 1..close];
    let tokens: Vec<&[u8]> = inner
        .split(|&b| is_ws(b))
        .filter(|t| !t.is_empty())
        .collect();
    if tokens != [b"0".as_slice(), SENTINEL, SENTINEL, SENTINEL] {
        return Err(SignError::Placeholder(format!(
            "/ByteRange is not the staging placeholder: [{}]",
            String::from_utf8_lossy(inner)
        )));
    }

    // /Contents <00…00>
    i = skip_ws(span, ct_keys[0] + b"/Contents".len());
    if span.get(i) != Some(&b'<') || span.get(i + 1) == Some(&b'<') {
        return Err(SignError::Placeholder(
            "/Contents is not a hex string".into(),
        ));
    }
    let c_open = i;
    let c_close = c_open
        + span[c_open..]
            .iter()
            .position(|&b| b == b'>')
            .ok_or_else(|| SignError::Placeholder("/Contents hex string is not closed".into()))?;
    let hex = &span[c_open + 1..c_close];
    if !hex.len().is_multiple_of(2) {
        return Err(SignError::Placeholder(format!(
            "/Contents placeholder has an odd length ({})",
            hex.len()
        )));
    }
    if hex.len() != expected_hex_len || hex.iter().any(|&b| b != b'0') {
        return Err(SignError::Placeholder(format!(
            "/Contents is not a {expected_hex_len}-digit zero placeholder"
        )));
    }

    let abs = base_len + start;
    Ok(Placeholder {
        byte_range_open: abs + open,
        byte_range_close: abs + close,
        contents_open: abs + c_open,
        contents_end: abs + c_close + 1,
    })
}

/// Write `range` into the `/ByteRange` slot in place, padding with spaces
/// so the file length and every later offset stay unchanged.
pub fn patch_byte_range(
    bytes: &mut [u8],
    ph: &Placeholder,
    range: [u64; 4],
) -> Result<(), SignError> {
    let width = ph.byte_range_close - ph.byte_range_open - 1;
    for v in range {
        if v.to_string().len() > SLOT_DIGITS {
            return Err(SignError::OffsetOverflow(v));
        }
    }
    let text = format!("{} {} {} {}", range[0], range[1], range[2], range[3]);
    if text.len() > width {
        return Err(SignError::OffsetOverflow(
            range.into_iter().max().unwrap_or(0),
        ));
    }
    let slot = &mut bytes[ph.byte_range_open + 1..ph.byte_range_close];
    slot.fill(b' ');
    slot[..text.len()].copy_from_slice(text.as_bytes());
    Ok(())
}

/// Validate a `/ByteRange` against the placeholder and file length:
/// starts at 0, ends at EOF, excludes exactly the `<...>` token.
pub fn check_byte_range(
    range: [u64; 4],
    ph: &Placeholder,
    file_len: usize,
) -> Result<(), SignError> {
    let [a, b, c, d] = range;
    let ok = a == 0
        && b == ph.contents_open as u64
        && c == ph.contents_end as u64
        && c.checked_add(d) == Some(file_len as u64);
    if !ok {
        return Err(SignError::Placeholder(format!(
            "malformed /ByteRange {range:?} for a {file_len}-byte file"
        )));
    }
    Ok(())
}

/// Hex-encode `der` into the `/Contents` placeholder (uppercase, zero
/// padded). `PlaceholderTooSmall.required` is in hex characters excluding
/// the delimiters (twice the DER length).
pub fn embed_contents(bytes: &mut [u8], ph: &Placeholder, der: &[u8]) -> Result<(), SignError> {
    let required = der.len() * 2;
    if required > ph.hex_len() {
        return Err(SignError::PlaceholderTooSmall { required });
    }
    let slot = &mut bytes[ph.contents_open + 1..ph.contents_end - 1];
    slot.fill(b'0');
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    for (i, b) in der.iter().enumerate() {
        slot[2 * i] = HEX[(b >> 4) as usize];
        slot[2 * i + 1] = HEX[(b & 0xf) as usize];
    }
    Ok(())
}
