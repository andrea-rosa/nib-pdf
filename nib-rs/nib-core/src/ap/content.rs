//! Typed PDF content-stream builder with 3-decimal number formatting.

use crate::model::Color;

/// `fmt_num` prints a float with up to 3 decimals, stripping trailing zeros
/// and a trailing `.`; `-0` normalizes to `0`.
pub fn fmt_num(v: f64) -> String {
    let v = if v == 0.0 { 0.0 } else { v };
    let s = format!("{:.3}", v);
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s.is_empty() || s == "-" {
        "0".to_string()
    } else {
        s.to_string()
    }
}

fn rgb(c: &Color) -> (f64, f64, f64) {
    let h = c.0.trim_start_matches('#');
    let p = |i: usize| u8::from_str_radix(&h[i..i + 2], 16).unwrap_or(0) as f64 / 255.0;
    (p(0), p(2), p(4))
}

/// Escape a PDF literal string: `(` `)` `\` and non-printable bytes become
/// octal/escape sequences.
fn escape_literal(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytes.len() + 2);
    for &b in bytes {
        match b {
            b'(' | b')' | b'\\' => {
                out.push(b'\\');
                out.push(b);
            }
            b'\n' => out.extend_from_slice(b"\\n"),
            b'\r' => out.extend_from_slice(b"\\r"),
            b'\t' => out.extend_from_slice(b"\\t"),
            0x08 => out.extend_from_slice(b"\\b"),
            0x0C => out.extend_from_slice(b"\\f"),
            0x20..=0x7E => out.push(b),
            _ => out.extend_from_slice(format!("\\{:03o}", b).as_bytes()),
        }
    }
    out
}

/// Accumulates PDF content-stream operators.
#[derive(Default)]
pub struct ContentBuilder {
    buf: Vec<u8>,
    depth: i32,
}

impl ContentBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    fn op(&mut self, args: &[&str], op: &str) {
        for a in args {
            self.buf.extend_from_slice(a.as_bytes());
            self.buf.push(b' ');
        }
        self.buf.extend_from_slice(op.as_bytes());
        self.buf.push(b'\n');
    }

    pub fn raw(&mut self, s: &str) {
        self.buf.extend_from_slice(s.as_bytes());
        self.buf.push(b'\n');
    }

    // -- graphics state ----------------------------------------------------

    pub fn save(&mut self) {
        self.depth += 1;
        self.op(&[], "q");
    }

    pub fn restore(&mut self) {
        self.depth -= 1;
        self.op(&[], "Q");
    }

    /// `true` when every `q` so far has a matching `Q`.
    pub fn balanced(&self) -> bool {
        self.depth == 0
    }

    pub fn gs(&mut self, name: &str) {
        self.op(&[&format!("/{name}")], "gs");
    }

    pub fn fill_rgb(&mut self, c: &Color) {
        let (r, g, b) = rgb(c);
        self.op(&[&fmt_num(r), &fmt_num(g), &fmt_num(b)], "rg");
    }

    pub fn stroke_rgb(&mut self, c: &Color) {
        let (r, g, b) = rgb(c);
        self.op(&[&fmt_num(r), &fmt_num(g), &fmt_num(b)], "RG");
    }

    pub fn line_width(&mut self, w: f64) {
        self.op(&[&fmt_num(w)], "w");
    }

    /// PDF line cap: 0 butt, 1 round, 2 square.
    pub fn line_cap(&mut self, cap: u8) {
        self.op(&[&cap.to_string()], "J");
    }

    /// PDF line join: 0 miter, 1 round, 2 bevel.
    pub fn line_join(&mut self, join: u8) {
        self.op(&[&join.to_string()], "j");
    }

    pub fn dash(&mut self, pattern: &[f64], phase: f64) {
        let arr = pattern
            .iter()
            .map(|v| fmt_num(*v))
            .collect::<Vec<_>>()
            .join(" ");
        self.op(&[&format!("[{arr}]"), &fmt_num(phase)], "d");
    }

    // -- path ---------------------------------------------------------------

    pub fn move_to(&mut self, x: f64, y: f64) {
        self.op(&[&fmt_num(x), &fmt_num(y)], "m");
    }

    pub fn line_to(&mut self, x: f64, y: f64) {
        self.op(&[&fmt_num(x), &fmt_num(y)], "l");
    }

    pub fn curve_to(&mut self, x1: f64, y1: f64, x2: f64, y2: f64, x3: f64, y3: f64) {
        self.op(
            &[
                &fmt_num(x1),
                &fmt_num(y1),
                &fmt_num(x2),
                &fmt_num(y2),
                &fmt_num(x3),
                &fmt_num(y3),
            ],
            "c",
        );
    }

    pub fn close(&mut self) {
        self.op(&[], "h");
    }

    pub fn rect(&mut self, x: f64, y: f64, w: f64, h: f64) {
        self.op(&[&fmt_num(x), &fmt_num(y), &fmt_num(w), &fmt_num(h)], "re");
    }

    pub fn stroke(&mut self) {
        self.op(&[], "S");
    }

    /// `f` — nonzero winding fill.
    pub fn fill(&mut self) {
        self.op(&[], "f");
    }

    pub fn fill_stroke(&mut self) {
        self.op(&[], "B");
    }

    /// End the path without painting (`n`).
    pub fn end_path(&mut self) {
        self.op(&[], "n");
    }

    /// `W n` — intersect clip with current path, end path.
    pub fn clip(&mut self) {
        self.op(&["W"], "n");
    }

    // -- text ----------------------------------------------------------------

    pub fn begin_text(&mut self) {
        self.op(&[], "BT");
    }

    pub fn end_text(&mut self) {
        self.op(&[], "ET");
    }

    pub fn font(&mut self, name: &str, size: f64) {
        self.op(&[&format!("/{name}"), &fmt_num(size)], "Tf");
    }

    /// `Td` — move the text position by (tx, ty) relative to the previous
    /// line start.
    pub fn text_pos(&mut self, tx: f64, ty: f64) {
        self.op(&[&fmt_num(tx), &fmt_num(ty)], "Td");
    }

    /// `Tj` — show bytes as a PDF literal string (already-encoded text).
    pub fn show_text(&mut self, bytes: &[u8]) {
        self.buf.push(b'(');
        self.buf.extend_from_slice(&escape_literal(bytes));
        self.buf.extend_from_slice(b") Tj\n");
    }

    // -- XObjects -------------------------------------------------------------

    /// `cm` — concatenate matrix to the CTM.
    pub fn cm(&mut self, a: f64, b: f64, c: f64, d: f64, e: f64, f: f64) {
        self.op(
            &[
                &fmt_num(a),
                &fmt_num(b),
                &fmt_num(c),
                &fmt_num(d),
                &fmt_num(e),
                &fmt_num(f),
            ],
            "cm",
        );
    }

    /// `Do` — paint the named XObject.
    pub fn do_xobject(&mut self, name: &str) {
        self.op(&[&format!("/{name}")], "Do");
    }

    pub fn finish(self) -> Vec<u8> {
        self.buf
    }
}
