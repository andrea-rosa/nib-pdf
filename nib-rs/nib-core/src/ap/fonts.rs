//! Base-14 PDF fonts: resource names, BaseFont names, and WinAnsi
//! (codes 32–255) glyph widths in 1/1000 em. Oblique/italic variants reuse
//! the upright widths (exact for Helvetica, close for Times); Courier is
//! 600 throughout.

use lopdf::{Dictionary, Object};

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum Base14 {
    Helvetica,
    HelveticaBold,
    HelveticaOblique,
    HelveticaBoldOblique,
    TimesRoman,
    TimesBold,
    TimesItalic,
    TimesBoldItalic,
    Courier,
    CourierBold,
    CourierOblique,
    CourierBoldOblique,
}

impl Base14 {
    /// Family names map onto the three Base-14 families; anything else
    /// (including `None`) is Helvetica.
    pub fn from_family(family: &str, bold: bool, italic: bool) -> Self {
        let f = family.to_ascii_lowercase();
        let times = f.contains("times");
        let courier = f.contains("courier") || f.contains("mono");
        match (times, courier, bold, italic) {
            (true, _, true, true) => Self::TimesBoldItalic,
            (true, _, true, false) => Self::TimesBold,
            (true, _, false, true) => Self::TimesItalic,
            (true, _, false, false) => Self::TimesRoman,
            (_, true, true, true) => Self::CourierBoldOblique,
            (_, true, true, false) => Self::CourierBold,
            (_, true, false, true) => Self::CourierOblique,
            (_, true, false, false) => Self::Courier,
            (_, _, true, true) => Self::HelveticaBoldOblique,
            (_, _, true, false) => Self::HelveticaBold,
            (_, _, false, true) => Self::HelveticaOblique,
            (_, _, false, false) => Self::Helvetica,
        }
    }

    /// True when `family` names a Base-14 family (or is empty).
    pub fn is_base14_family(family: &str) -> bool {
        let f = family.trim().to_ascii_lowercase();
        f.is_empty()
            || f.contains("helvetica")
            || f.contains("arial")
            || f.contains("times")
            || f.contains("courier")
            || f.contains("mono")
    }

    /// Resource name used inside the appearance stream (`/Helv …`).
    pub fn resource_name(&self) -> &'static str {
        match self {
            Self::Helvetica => "Helv",
            Self::HelveticaBold => "HeBo",
            Self::HelveticaOblique => "HeOb",
            Self::HelveticaBoldOblique => "HeBO",
            Self::TimesRoman => "TiRo",
            Self::TimesBold => "TiBo",
            Self::TimesItalic => "TiIt",
            Self::TimesBoldItalic => "TiBI",
            Self::Courier => "Cour",
            Self::CourierBold => "CoBo",
            Self::CourierOblique => "CoOb",
            Self::CourierBoldOblique => "CoBO",
        }
    }

    pub fn base_font(&self) -> &'static str {
        match self {
            Self::Helvetica => "Helvetica",
            Self::HelveticaBold => "Helvetica-Bold",
            Self::HelveticaOblique => "Helvetica-Oblique",
            Self::HelveticaBoldOblique => "Helvetica-BoldOblique",
            Self::TimesRoman => "Times-Roman",
            Self::TimesBold => "Times-Bold",
            Self::TimesItalic => "Times-Italic",
            Self::TimesBoldItalic => "Times-BoldItalic",
            Self::Courier => "Courier",
            Self::CourierBold => "Courier-Bold",
            Self::CourierOblique => "Courier-Oblique",
            Self::CourierBoldOblique => "Courier-BoldOblique",
        }
    }

    /// WinAnsi widths for codes 32–255 (index 0 = code 32).
    pub fn widths(&self) -> &'static [u16; 224] {
        match self {
            Self::Helvetica | Self::HelveticaOblique => &HELVETICA,
            Self::HelveticaBold | Self::HelveticaBoldOblique => &HELVETICA_BOLD,
            Self::TimesRoman | Self::TimesItalic => &TIMES_ROMAN,
            Self::TimesBold | Self::TimesBoldItalic => &TIMES_BOLD,
            Self::Courier | Self::CourierBold | Self::CourierOblique | Self::CourierBoldOblique => {
                &COURIER
            }
        }
    }

    /// Advance width of `s` (already WinAnsi-encoded) at `size` pt.
    pub fn text_width(&self, s: &[u8], size: f64) -> f64 {
        let w = &self.widths();
        let sum: u64 = s
            .iter()
            .map(|&b| {
                if (32..=255).contains(&b) {
                    w[(b - 32) as usize] as u64
                } else {
                    w[b'?' as usize - 32] as u64
                }
            })
            .sum();
        sum as f64 * size / 1000.0
    }

    /// `<< /Type /Font /Subtype /Type1 /BaseFont /… /Encoding /WinAnsiEncoding >>`
    pub fn font_dict(&self) -> Dictionary {
        let mut d = Dictionary::new();
        d.set("Type", Object::Name(b"Font".to_vec()));
        d.set("Subtype", Object::Name(b"Type1".to_vec()));
        d.set("BaseFont", Object::Name(self.base_font().into()));
        d.set("Encoding", Object::Name(b"WinAnsiEncoding".to_vec()));
        d
    }
}

/// Encode a string to WinAnsi bytes; unencodable characters become `?`.
/// Returns `(bytes, had_unencodable)`.
pub fn to_winansi(s: &str) -> (Vec<u8>, bool) {
    let mut out = Vec::with_capacity(s.len());
    let mut bad = false;
    for c in s.chars() {
        match c {
            '\u{20}'..='\u{7E}' | '\u{A0}'..='\u{FF}' => out.push(c as u8),
            '€' => out.push(0x80),
            '‚' => out.push(0x82),
            'ƒ' => out.push(0x83),
            '„' => out.push(0x84),
            '…' => out.push(0x85),
            '†' => out.push(0x86),
            '‡' => out.push(0x87),
            'ˆ' => out.push(0x88),
            '‰' => out.push(0x89),
            'Š' => out.push(0x8A),
            '‹' => out.push(0x8B),
            'Œ' => out.push(0x8C),
            'Ž' => out.push(0x8E),
            '‘' => out.push(0x91),
            '’' => out.push(0x92),
            '“' => out.push(0x93),
            '”' => out.push(0x94),
            '•' => out.push(0x95),
            '–' => out.push(0x96),
            '—' => out.push(0x97),
            '˜' => out.push(0x98),
            '™' => out.push(0x99),
            'š' => out.push(0x9A),
            '›' => out.push(0x9B),
            'œ' => out.push(0x9C),
            'ž' => out.push(0x9E),
            'Ÿ' => out.push(0x9F),
            _ => {
                bad = true;
                out.push(b'?');
            }
        }
    }
    (out, bad)
}

const fn make(ascii: &[u16; 95], spec: &[u16; 32], latin1: &[u16; 96]) -> [u16; 224] {
    let mut w = [0u16; 224];
    let mut i = 0;
    while i < 95 {
        w[i] = ascii[i];
        i += 1;
    }
    w[95] = 0; // code 127
    i = 0;
    while i < 32 {
        w[96 + i] = spec[i];
        i += 1;
    }
    i = 0;
    while i < 96 {
        w[128 + i] = latin1[i];
        i += 1;
    }
    w
}

// Codes 32–126.
const HELV_ASCII: [u16; 95] = [
    278, 278, 355, 556, 556, 889, 667, 191, 333, 333, 389, 584, 278, 333, 278, 278, // 32-47
    556, 556, 556, 556, 556, 556, 556, 556, 556, 556, // 0-9
    278, 278, 584, 584, 584, 556, 1015, // : ; < = > ? @
    667, 667, 722, 722, 667, 611, 778, 722, 278, 500, 667, 556, 833, 722, 778, 667, 778, 722, 667,
    611, 722, 667, 944, 667, 667, 611, // A-Z
    278, 278, 278, 469, 556, 333, // [ \ ] ^ _ `
    556, 556, 500, 556, 556, 278, 556, 556, 222, 222, 500, 222, 833, 556, 556, 556, 556, 333, 500,
    278, 556, 500, 722, 500, 500, 500, // a-z
    334, 260, 334, 584, // { | } ~
];

const HELV_BOLD_ASCII: [u16; 95] = [
    278, 333, 474, 556, 556, 889, 722, 238, 333, 333, 389, 584, 278, 333, 278, 278, 556, 556, 556,
    556, 556, 556, 556, 556, 556, 556, 333, 333, 584, 584, 584, 611, 975, 722, 722, 722, 722, 667,
    611, 778, 722, 278, 556, 722, 611, 833, 722, 778, 667, 778, 722, 667, 611, 722, 667, 944, 667,
    667, 611, 333, 278, 333, 584, 556, 333, 556, 611, 556, 611, 556, 333, 611, 611, 278, 278, 556,
    278, 889, 611, 611, 611, 611, 389, 556, 333, 611, 556, 778, 556, 556, 500, 389, 280, 389, 584,
];

const TIMES_ASCII: [u16; 95] = [
    250, 333, 408, 500, 500, 833, 778, 180, 333, 333, 500, 564, 250, 333, 250, 278, 500, 500, 500,
    500, 500, 500, 500, 500, 500, 500, 278, 278, 564, 564, 564, 444, 921, 722, 667, 667, 722, 611,
    556, 722, 722, 333, 389, 722, 611, 889, 722, 722, 556, 722, 667, 556, 611, 722, 722, 944, 722,
    722, 611, 333, 278, 333, 469, 500, 333, 444, 500, 444, 500, 444, 333, 500, 500, 278, 278, 500,
    278, 778, 500, 500, 500, 500, 333, 389, 278, 500, 500, 722, 500, 500, 444, 480, 200, 480, 541,
];

const TIMES_BOLD_ASCII: [u16; 95] = [
    250, 333, 555, 500, 500, 1000, 833, 278, 333, 333, 500, 570, 250, 333, 250, 278, 500, 500, 500,
    500, 500, 500, 500, 500, 500, 500, 333, 333, 570, 570, 570, 500, 930, 722, 667, 722, 722, 667,
    611, 778, 778, 389, 500, 778, 667, 944, 722, 778, 611, 778, 722, 556, 667, 722, 722, 1000, 722,
    722, 667, 333, 278, 333, 581, 500, 333, 500, 556, 444, 556, 444, 333, 500, 556, 278, 333, 556,
    278, 833, 556, 500, 556, 556, 444, 389, 333, 556, 500, 722, 500, 500, 444, 394, 220, 394, 520,
];

// Codes 128–159 (unused → 0).
const HELV_SPEC: [u16; 32] = [
    556, 0, 222, 556, 333, 1000, 556, 556, 333, 1000, 667, 333, 1000, 0, 611, 0, 0, 222, 222, 333,
    333, 350, 556, 1000, 333, 1000, 500, 333, 944, 0, 500, 667,
];

const TIMES_SPEC: [u16; 32] = [
    500, 0, 333, 500, 444, 1000, 500, 500, 333, 1000, 556, 333, 889, 0, 611, 0, 0, 333, 333, 444,
    444, 350, 500, 1000, 333, 980, 389, 333, 722, 0, 444, 722,
];

// Codes 160–255.
const HELV_LATIN1: [u16; 96] = [
    278, 333, 556, 556, 556, 556, 260, 556, 333, 737, 370, 556, 584, 333, 737, 333, 400, 584, 333,
    333, 333, 556, 537, 278, 333, 333, 365, 556, 834, 834, 834, 611, 667, 667, 667, 667, 667, 667,
    1000, 722, 667, 667, 667, 667, 278, 278, 278, 278, 722, 722, 778, 778, 778, 778, 778, 584, 778,
    722, 722, 722, 722, 667, 667, 611, 556, 556, 556, 556, 556, 556, 889, 500, 556, 556, 556, 556,
    222, 222, 222, 222, 556, 556, 556, 556, 556, 556, 556, 584, 611, 556, 556, 556, 556, 500, 556,
    500,
];

const HELV_BOLD_LATIN1: [u16; 96] = [
    278, 333, 556, 556, 556, 556, 280, 556, 333, 737, 370, 556, 584, 333, 737, 333, 400, 584, 333,
    333, 333, 611, 537, 278, 333, 333, 365, 556, 834, 834, 834, 611, 722, 722, 722, 722, 722, 722,
    1000, 722, 667, 667, 667, 667, 278, 278, 278, 278, 722, 722, 778, 778, 778, 778, 778, 584, 778,
    722, 722, 722, 722, 667, 667, 611, 556, 556, 556, 556, 556, 556, 889, 556, 556, 556, 556, 556,
    278, 278, 278, 278, 611, 611, 611, 611, 611, 611, 611, 584, 611, 611, 611, 611, 611, 556, 611,
    556,
];

const TIMES_LATIN1: [u16; 96] = [
    250, 333, 500, 500, 500, 500, 200, 500, 333, 760, 276, 500, 564, 333, 760, 333, 400, 564, 300,
    300, 333, 500, 453, 250, 333, 300, 310, 500, 750, 750, 750, 444, 722, 722, 722, 722, 722, 722,
    889, 667, 611, 611, 611, 611, 333, 333, 333, 333, 722, 722, 722, 722, 722, 722, 722, 564, 722,
    722, 722, 722, 722, 722, 556, 500, 444, 444, 444, 444, 444, 444, 667, 444, 444, 444, 444, 444,
    278, 278, 278, 278, 500, 500, 500, 500, 500, 500, 500, 564, 500, 500, 500, 500, 500, 500, 500,
    500,
];

const TIMES_BOLD_LATIN1: [u16; 96] = [
    250, 333, 500, 500, 500, 500, 220, 500, 333, 747, 300, 500, 570, 333, 747, 333, 400, 570, 300,
    300, 333, 556, 540, 250, 333, 300, 330, 500, 750, 750, 750, 500, 722, 722, 722, 722, 722, 722,
    1000, 722, 667, 667, 667, 667, 389, 389, 389, 389, 722, 722, 778, 778, 778, 778, 778, 570, 778,
    722, 722, 722, 722, 722, 611, 556, 500, 500, 500, 500, 500, 500, 889, 444, 444, 444, 444, 444,
    278, 278, 278, 278, 556, 556, 500, 500, 500, 500, 500, 570, 500, 556, 556, 556, 556, 500, 556,
    500,
];

const HELVETICA: [u16; 224] = make(&HELV_ASCII, &HELV_SPEC, &HELV_LATIN1);
const HELVETICA_BOLD: [u16; 224] = make(&HELV_BOLD_ASCII, &HELV_SPEC, &HELV_BOLD_LATIN1);
const TIMES_ROMAN: [u16; 224] = make(&TIMES_ASCII, &TIMES_SPEC, &TIMES_LATIN1);
const TIMES_BOLD: [u16; 224] = make(&TIMES_BOLD_ASCII, &TIMES_SPEC, &TIMES_BOLD_LATIN1);
const COURIER: [u16; 224] = [600; 224];
