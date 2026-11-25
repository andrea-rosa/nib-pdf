use std::collections::{HashMap, HashSet};

use sha2::{Digest, Sha256};
use skrifa::{
    instance::{LocationRef, Size},
    prelude::MetadataProvider,
    raw::TableProvider,
    FontRef, GlyphId,
};
use subsetter::GlyphRemapper;

pub const RESOURCE_NAME: &str = "NIBF";

pub struct EmbeddedFont {
    pub base_font: String,
    pub program: Vec<u8>,
    pub font_bbox: [i32; 4],
    pub ascent: i32,
    pub descent: i32,
    pub cap_height: i32,
    pub widths: Vec<i32>,
    pub cid_to_gid: Vec<u8>,
    pub to_unicode: Vec<u8>,
}

pub struct PreparedFont {
    pub resource: EmbeddedFont,
    char_to_cid: HashMap<char, u16>,
    advance_units: HashMap<char, f32>,
    units_per_em: u16,
    pub missing_glyphs: usize,
}

impl PreparedFont {
    pub fn encode(&self, text: &str, size: f64) -> (Vec<u8>, f64) {
        let mut bytes = Vec::with_capacity(text.len() * 2);
        let mut width = 0.0;
        for ch in text.chars() {
            let cid = self.char_to_cid[&ch];
            bytes.extend_from_slice(&cid.to_be_bytes());
            width += self.advance_units[&ch] as f64 * size / self.units_per_em as f64;
        }
        (bytes, width)
    }

    pub fn wrap_lines(&self, text: &str, size: f64, max_width: f64) -> Vec<(Vec<u8>, f64)> {
        let mut lines = Vec::new();
        let space_width = self.advance_units.get(&' ').copied().unwrap_or_default() as f64 * size
            / self.units_per_em as f64;

        for paragraph in text.split('\n') {
            let mut line = Vec::new();
            let mut line_width = 0.0;
            for word in paragraph.split(' ') {
                let (word_bytes, word_width) = self.encode(word, size);
                let needed = word_width + if line.is_empty() { 0.0 } else { space_width };
                if line_width + needed > max_width && !line.is_empty() {
                    lines.push((std::mem::take(&mut line), line_width));
                    line_width = 0.0;
                }

                if word_width > max_width {
                    for ch in word.chars() {
                        let (char_bytes, char_width) = self.encode(&ch.to_string(), size);
                        if line_width + char_width > max_width && !line.is_empty() {
                            lines.push((std::mem::take(&mut line), line_width));
                            line_width = 0.0;
                        }
                        line.extend_from_slice(&char_bytes);
                        line_width += char_width;
                    }
                } else {
                    if !line.is_empty() {
                        line.extend_from_slice(&self.char_to_cid[&' '].to_be_bytes());
                        line_width += space_width;
                    }
                    line.extend_from_slice(&word_bytes);
                    line_width += word_width;
                }
            }
            lines.push((line, line_width));
        }
        lines
    }
}

pub fn prepare(data: &[u8], text: &str) -> Result<PreparedFont, String> {
    if data.starts_with(b"ttcf") {
        return Err("TrueType collections are not supported".into());
    }
    let font = FontRef::new(data).map_err(|e| format!("invalid TrueType font: {e}"))?;
    let head = font
        .head()
        .map_err(|e| format!("TrueType font has no valid head table: {e}"))?;
    let hhea = font
        .hhea()
        .map_err(|e| format!("TrueType font has no valid hhea table: {e}"))?;
    let os2 = font
        .os2()
        .map_err(|e| format!("TrueType font has no valid OS/2 table: {e}"))?;

    font.glyf()
        .map_err(|_| "font is not a supported TrueType outline font".to_string())?;
    font.loca(None)
        .map_err(|_| "font is missing a valid TrueType loca table".to_string())?;
    if font.fvar().is_ok() {
        return Err("variable TrueType fonts are not supported".into());
    }

    let fs_type = os2.fs_type();
    if fs_type & 0x0002 != 0 {
        return Err("font embedding is restricted by its OS/2 fsType".into());
    }
    if fs_type & 0x0100 != 0 {
        return Err("font license prohibits subsetting".into());
    }
    if fs_type & 0x0200 != 0 {
        return Err("font license allows bitmap embedding only".into());
    }

    let units_per_em = head.units_per_em();
    if units_per_em == 0 {
        return Err("font has invalid units per em".into());
    }
    let charmap = font.charmap();
    if !charmap.has_map() {
        return Err("font has no supported Unicode character map".into());
    }
    let metrics = font.glyph_metrics(Size::unscaled(), LocationRef::default());
    let glyph_count = metrics.glyph_count();
    if glyph_count == 0 || glyph_count > u16::MAX as u32 {
        return Err("font has an unsupported glyph count".into());
    }

    let mut chars = Vec::new();
    let mut seen = HashSet::new();
    let mut occurrences = HashMap::new();
    for ch in text.chars().filter(|ch| *ch != '\n') {
        *occurrences.entry(ch).or_insert(0usize) += 1;
        if seen.insert(ch) {
            chars.push(ch);
        }
    }
    if chars.len() > u16::MAX as usize - 1 {
        return Err("text requires more than 65,534 distinct character codes".into());
    }

    let mut source_glyphs = Vec::with_capacity(chars.len());
    let mut char_to_cid = HashMap::with_capacity(chars.len());
    let mut advance_units = HashMap::with_capacity(chars.len());
    let mut missing_glyphs = 0;
    let mut remapper = GlyphRemapper::new();

    for (index, ch) in chars.iter().copied().enumerate() {
        let glyph_id = charmap
            .map(ch)
            .filter(|gid| gid.to_u32() > 0 && gid.to_u32() < glyph_count);
        let source_gid = glyph_id.map_or(0, |gid| gid.to_u32() as u16);
        if glyph_id.is_none() {
            missing_glyphs += occurrences.get(&ch).copied().unwrap_or_default();
        }
        remapper.remap(source_gid);
        let advance = metrics
            .advance_width(GlyphId::new(source_gid as u32))
            .ok_or_else(|| format!("font is missing metrics for U+{:04X}", ch as u32))?;
        if !advance.is_finite() || advance < 0.0 {
            return Err(format!("font has invalid advance for U+{:04X}", ch as u32));
        }
        let cid = (index + 1) as u16;
        char_to_cid.insert(ch, cid);
        advance_units.insert(ch, advance);
        source_glyphs.push((ch, source_gid));
    }

    let program = subsetter::subset(data, 0, &remapper)
        .map_err(|e| format!("TrueType subsetting failed: {e}"))?;
    let mut cid_to_gid = vec![0; (chars.len() + 1) * 2];
    let mut widths = Vec::with_capacity(chars.len());
    for (ch, source_gid) in source_glyphs {
        let cid = char_to_cid[&ch] as usize;
        let subset_gid = remapper
            .get(source_gid)
            .ok_or_else(|| format!("subset omitted glyph for U+{:04X}", ch as u32))?;
        cid_to_gid[cid * 2..cid * 2 + 2].copy_from_slice(&subset_gid.to_be_bytes());
        let width = (advance_units[&ch] as f64 * 1000.0 / units_per_em as f64).round();
        widths.push(width as i32);
    }

    let digest = Sha256::digest(&program);
    let prefix: String = digest[..6]
        .iter()
        .map(|byte| (b'A' + byte % 26) as char)
        .collect();
    let base_font = format!("{prefix}+NibFont");
    let to_unicode = make_to_unicode(&chars, &char_to_cid);
    let scale = 1000.0 / units_per_em as f64;

    Ok(PreparedFont {
        resource: EmbeddedFont {
            base_font,
            program,
            font_bbox: [
                (head.x_min() as f64 * scale).round() as i32,
                (head.y_min() as f64 * scale).round() as i32,
                (head.x_max() as f64 * scale).round() as i32,
                (head.y_max() as f64 * scale).round() as i32,
            ],
            ascent: (hhea.ascender().to_i16() as f64 * scale).round() as i32,
            descent: (hhea.descender().to_i16() as f64 * scale).round() as i32,
            cap_height: os2
                .s_cap_height()
                .map(|height| (height as f64 * scale).round() as i32)
                .unwrap_or_else(|| (hhea.ascender().to_i16() as f64 * scale).round() as i32),
            widths,
            cid_to_gid,
            to_unicode,
        },
        char_to_cid,
        advance_units,
        units_per_em,
        missing_glyphs,
    })
}

fn make_to_unicode(chars: &[char], char_to_cid: &HashMap<char, u16>) -> Vec<u8> {
    let mut cmap = String::from(
        "/CIDInit /ProcSet findresource begin\n\
         12 dict begin\n\
         begincmap\n\
         /CIDSystemInfo << /Registry (Adobe) /Ordering (UCS) /Supplement 0 >> def\n\
         /CMapName /Adobe-Identity-UCS def\n\
         /CMapType 2 def\n\
         1 begincodespacerange\n\
         <0000> <FFFF>\n\
         endcodespacerange\n",
    );
    for chunk in chars.chunks(100) {
        cmap.push_str(&format!("{} beginbfchar\n", chunk.len()));
        for ch in chunk {
            let cid = char_to_cid[ch];
            let utf16: String = ch
                .encode_utf16(&mut [0; 2])
                .iter()
                .map(|unit| format!("{unit:04X}"))
                .collect();
            cmap.push_str(&format!("<{cid:04X}> <{utf16}>\n"));
        }
        cmap.push_str("endbfchar\n");
    }
    cmap.push_str(
        "endcmap\n\
         CMapName currentdict /CMap defineresource pop\n\
         end\n\
         end\n",
    );
    cmap.into_bytes()
}
