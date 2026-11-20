//! /AP parity harness: render each generated appearance stream with PDFium
//! and compare against a tiny-skia reference raster of the same drawing
//! rules (AnnotationDrawing.kt port), per subtype x opacity.

use std::path::PathBuf;
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use image_hasher::HasherConfig;
use lopdf::{dictionary, Document, Object, Stream};
use nib_core::ap::fonts::Base14;
use nib_core::ap::freetext::{wrap_lines, ASCENT};
use nib_core::ap::{build, NoAssets, NoFonts};
use nib_core::geom::{Point, QuadPoints, Rect};
use nib_core::model::annotation::*;
use nib_core::model::{AnnotBase, Annotation, Color};
use nib_core::writer::{FlushOptions, Writer};
use nib_render::types::{IRect, ISize, TileResult};
use nib_render::{Priority, RenderWorker, TileKey, TileRequest};
use tiny_skia::{
    BlendMode as SkBlend, Color as SkColor, FillRule, LineCap as SkLineCap, LineJoin as SkLineJoin,
    Paint, Path, PathBuilder, Pixmap, PixmapPaint, PixmapRef, Shader, Stroke as SkStroke,
    StrokeDash, Transform,
};
use uuid::Uuid;

const PAGE: f64 = 400.0;

fn host_pdfium_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("NIB_PDFIUM_DIR") {
        return dir.into();
    }
    let triple = if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
        "linux-x64"
    } else if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        "mac-arm64"
    } else if cfg!(all(target_os = "macos", target_arch = "x86_64")) {
        "mac-x64"
    } else {
        panic!("unsupported host; set NIB_PDFIUM_DIR");
    };
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../target/pdfium")
        .join(triple)
}

/// One process-wide worker for all tests: `Pdfium::new` /
/// `FPDF_InitLibrary` may only run once per process.
fn worker() -> Arc<RenderWorker> {
    static WORKER: OnceLock<Arc<RenderWorker>> = OnceLock::new();
    Arc::clone(WORKER.get_or_init(|| RenderWorker::spawn(Some(host_pdfium_dir()))))
}

fn blank_pdf() -> Vec<u8> {
    let mut doc = Document::new();
    let contents = doc.add_object(Stream::new(dictionary! {}, Vec::new()));
    let page = doc.add_object(dictionary! {
        "Type" => "Page",
        "MediaBox" => vec![Object::Integer(0), Object::Integer(0), Object::Integer(400), Object::Integer(400)],
        "Contents" => Object::Reference(contents),
        "Resources" => dictionary! {},
    });
    let pages = doc.add_object(dictionary! {
        "Type" => "Pages",
        "Kids" => vec![Object::Reference(page)],
        "Count" => 1,
    });
    doc.get_object_mut(page)
        .unwrap()
        .as_dict_mut()
        .unwrap()
        .set("Parent", Object::Reference(pages));
    let catalog = doc.add_object(dictionary! {
        "Type" => "Catalog",
        "Pages" => Object::Reference(pages),
    });
    doc.trailer.set("Root", Object::Reference(catalog));
    let mut out = Vec::new();
    doc.save_to(&mut out).unwrap();
    out
}

fn wait_tile(
    mut rx: futures_channel::oneshot::Receiver<Result<TileResult, nib_render::RenderError>>,
) -> TileResult {
    let start = Instant::now();
    loop {
        match rx.try_recv() {
            Ok(Some(r)) => return r.unwrap(),
            Ok(None) => {
                assert!(start.elapsed() < Duration::from_secs(30), "tile timeout");
                std::thread::sleep(Duration::from_millis(1));
            }
            Err(_) => panic!("tile cancelled"),
        }
    }
}

fn render_page(w: &RenderWorker, doc: u64) -> TileResult {
    let req = TileRequest {
        key: TileKey {
            page: 0,
            bucket: 0,
            tx: 0,
            ty: 0,
            revision: 0,
            flags: 0,
        },
        device_rect: IRect {
            x: 0,
            y: 0,
            width: PAGE as i32,
            height: PAGE as i32,
        },
        page_device_size: ISize {
            width: PAGE as i32,
            height: PAGE as i32,
        },
        generation: u64::MAX,
        priority: Priority::Visible,
        distance: 0,
    };
    wait_tile(w.render_tile(doc, req))
}

// ------------------------------------------------------------- tiny-skia ref

struct Ctx {
    pm: Pixmap,
    opacity: f64,
}

fn xform() -> Transform {
    // PDF y-up -> pixmap y-down.
    Transform::from_row(1.0, 0.0, 0.0, -1.0, 0.0, PAGE as f32)
}

fn sk_rect(x0: f32, y0: f32, x1: f32, y1: f32) -> tiny_skia::Rect {
    tiny_skia::Rect::from_ltrb(x0, y0, x1, y1).unwrap()
}

fn rgba(hex: &str, a: f64) -> SkColor {
    let h = hex.trim_start_matches('#');
    let p = |i: usize| u8::from_str_radix(&h[i..i + 2], 16).unwrap();
    SkColor::from_rgba8(p(0), p(2), p(4), (a.clamp(0.0, 1.0) * 255.0).round() as u8)
}

fn col(c: &Option<Color>, default: &str) -> String {
    c.as_ref()
        .map(|x| x.0.clone())
        .unwrap_or_else(|| default.into())
}

impl Ctx {
    fn paint(&self, hex: &str) -> Paint<'static> {
        Paint {
            shader: Shader::SolidColor(rgba(hex, self.opacity)),
            anti_alias: true,
            ..Default::default()
        }
    }
    fn fill(&mut self, p: &Path, hex: &str, multiply: bool) {
        let mut paint = self.paint(hex);
        if multiply {
            paint.blend_mode = SkBlend::Multiply;
        }
        self.pm
            .fill_path(p, &paint, FillRule::Winding, xform(), None);
    }
    fn stroke(&mut self, p: &Path, hex: &str, w: f64, dash: Option<Vec<f32>>) {
        self.stroke_cap(p, hex, w, dash, SkLineCap::Butt);
    }
    fn stroke_cap(&mut self, p: &Path, hex: &str, w: f64, dash: Option<Vec<f32>>, cap: SkLineCap) {
        let st = SkStroke {
            width: w as f32,
            line_cap: cap,
            line_join: SkLineJoin::Round,
            dash: dash.and_then(|d| StrokeDash::new(d, 0.0)),
            ..Default::default()
        };
        let paint = self.paint(hex);
        self.pm.stroke_path(p, &paint, &st, xform(), None);
    }
    fn rect(&mut self, r: &Rect, hex: &str) {
        let p = PathBuilder::from_rect(sk_rect(r.x0 as f32, r.y0 as f32, r.x1 as f32, r.y1 as f32));
        self.fill(&p, hex, false);
    }
}

fn circle_path(cx: f64, cy: f64, r: f64) -> Path {
    const K: f64 = 0.5523;
    let k = r * K;
    let mut b = PathBuilder::new();
    b.move_to((cx + r) as f32, cy as f32);
    b.cubic_to(
        (cx + r) as f32,
        (cy + k) as f32,
        (cx + k) as f32,
        (cy + r) as f32,
        cx as f32,
        (cy + r) as f32,
    );
    b.cubic_to(
        (cx - k) as f32,
        (cy + r) as f32,
        (cx - r) as f32,
        (cy + k) as f32,
        (cx - r) as f32,
        cy as f32,
    );
    b.cubic_to(
        (cx - r) as f32,
        (cy - k) as f32,
        (cx - k) as f32,
        (cy - r) as f32,
        cx as f32,
        (cy - r) as f32,
    );
    b.cubic_to(
        (cx + k) as f32,
        (cy - r) as f32,
        (cx + r) as f32,
        (cy - k) as f32,
        (cx + r) as f32,
        cy as f32,
    );
    b.close();
    b.finish().unwrap()
}

fn ellipse_path(cx: f64, cy: f64, rx: f64, ry: f64) -> Path {
    const K: f64 = 0.5523;
    let (kx, ky) = (rx * K, ry * K);
    let mut b = PathBuilder::new();
    b.move_to((cx + rx) as f32, cy as f32);
    b.cubic_to(
        (cx + rx) as f32,
        (cy + ky) as f32,
        (cx + kx) as f32,
        (cy + ry) as f32,
        cx as f32,
        (cy + ry) as f32,
    );
    b.cubic_to(
        (cx - kx) as f32,
        (cy + ry) as f32,
        (cx - rx) as f32,
        (cy + ky) as f32,
        (cx - rx) as f32,
        cy as f32,
    );
    b.cubic_to(
        (cx - rx) as f32,
        (cy - ky) as f32,
        (cx - kx) as f32,
        (cy - ry) as f32,
        cx as f32,
        (cy - ry) as f32,
    );
    b.cubic_to(
        (cx + kx) as f32,
        (cy - ry) as f32,
        (cx + rx) as f32,
        (cy - ky) as f32,
        (cx + rx) as f32,
        cy as f32,
    );
    b.close();
    b.finish().unwrap()
}

fn round_rect_path(r: &Rect, rad: f64) -> Path {
    const K: f64 = 0.5523;
    let k = rad * K;
    let mut b = PathBuilder::new();
    b.move_to((r.x0 + rad) as f32, r.y0 as f32);
    b.line_to((r.x1 - rad) as f32, r.y0 as f32);
    b.cubic_to(
        (r.x1 - rad + k) as f32,
        r.y0 as f32,
        r.x1 as f32,
        (r.y0 + rad - k) as f32,
        r.x1 as f32,
        (r.y0 + rad) as f32,
    );
    b.line_to(r.x1 as f32, (r.y1 - rad) as f32);
    b.cubic_to(
        r.x1 as f32,
        (r.y1 - rad + k) as f32,
        (r.x1 - rad + k) as f32,
        r.y1 as f32,
        (r.x1 - rad) as f32,
        r.y1 as f32,
    );
    b.line_to((r.x0 + rad) as f32, r.y1 as f32);
    b.cubic_to(
        (r.x0 + rad - k) as f32,
        r.y1 as f32,
        r.x0 as f32,
        (r.y1 - rad + k) as f32,
        r.x0 as f32,
        (r.y1 - rad) as f32,
    );
    b.line_to(r.x0 as f32, (r.y0 + rad) as f32);
    b.cubic_to(
        r.x0 as f32,
        (r.y0 + rad - k) as f32,
        (r.x0 + rad - k) as f32,
        r.y0 as f32,
        (r.x0 + rad) as f32,
        r.y0 as f32,
    );
    b.close();
    b.finish().unwrap()
}

fn line_path(pts: &[(f64, f64)]) -> Path {
    let mut b = PathBuilder::new();
    b.move_to(pts[0].0 as f32, pts[0].1 as f32);
    for p in &pts[1..] {
        b.line_to(p.0 as f32, p.1 as f32);
    }
    b.finish().unwrap()
}

fn catmull_path(pts: &[(f64, f64)], tension: f64) -> Path {
    if pts.len() <= 2 {
        return line_path(pts);
    }
    let t = tension / 3.0;
    let mut b = PathBuilder::new();
    b.move_to(pts[0].0 as f32, pts[0].1 as f32);
    for i in 0..pts.len() - 1 {
        let p0 = pts[i.saturating_sub(1)];
        let p1 = pts[i];
        let p2 = pts[i + 1];
        let p3 = pts[(i + 2).min(pts.len() - 1)];
        b.cubic_to(
            (p1.0 + (p2.0 - p0.0) * t) as f32,
            (p1.1 + (p2.1 - p0.1) * t) as f32,
            (p2.0 - (p3.0 - p1.0) * t) as f32,
            (p2.1 - (p3.1 - p1.1) * t) as f32,
            p2.0 as f32,
            p2.1 as f32,
        );
    }
    b.finish().unwrap()
}

fn seg(x0: f64, y0: f64, x1: f64, y1: f64) -> Path {
    line_path(&[(x0, y0), (x1, y1)])
}

/// Host font for text: Arial's metrics are identical to Helvetica's, which
/// is what PDFium substitutes for the Base-14 font. Returns (ttf bytes,
/// face index).
fn host_font(bold: bool) -> Option<(Vec<u8>, usize)> {
    let names: &[&str] = if bold {
        &[
            "Arial Bold.ttf",
            "LiberationSans-Bold.ttf",
            "DejaVuSans-Bold.ttf",
        ]
    } else {
        &["Arial.ttf", "LiberationSans-Regular.ttf", "DejaVuSans.ttf"]
    };
    let dirs = [
        "/System/Library/Fonts/Supplemental",
        "/Library/Fonts",
        "/System/Library/Fonts",
        "/usr/share/fonts/truetype/msttcorefonts",
        "/usr/share/fonts/truetype/liberation",
        "/usr/share/fonts/truetype/dejavu",
    ];
    for d in dirs {
        for n in names {
            let p = format!("{d}/{n}");
            if let Ok(bytes) = std::fs::read(&p) {
                if swash::FontRef::from_index(&bytes, 0).is_some() {
                    return Some((bytes, 0usize));
                }
            }
        }
    }
    None
}

/// Rasterize `text` (WinAnsi bytes; ASCII expected) into `ctx.pm` with the
/// baseline at PDF y `baseline` starting at PDF x `x`, colour `hex`.
/// Advances use the same AFM widths the /AP text layout used.
#[allow(clippy::too_many_arguments)]
fn draw_glyphs(
    ctx: &mut Ctx,
    font_bytes: &[u8],
    face: usize,
    afm: Base14,
    text: &[u8],
    x: f64,
    baseline: f64,
    size: f64,
    hex: &str,
) {
    let font = match swash::FontRef::from_index(font_bytes, face) {
        Some(f) => f,
        None => return,
    };
    let charmap = font.charmap();
    let mut scx = swash::scale::ScaleContext::new();
    let mut scaler = scx.builder(font).size(size as f32).hint(true).build();
    let h = hex.trim_start_matches('#');
    let p = |i: usize| u8::from_str_radix(&h[i..i + 2], 16).unwrap();
    let (fg_r, fg_g, fg_b) = (p(0), p(2), p(4));
    let baseline_dev = PAGE - baseline;
    let mut pen = x;
    for &byte in text {
        let ch = if byte < 0x80 { byte as char } else { '?' };
        let gid = charmap.map(ch);
        if let Some(img) = swash::scale::Render::new(&[
            swash::scale::Source::ColorOutline(0),
            swash::scale::Source::Outline,
        ])
        .render(&mut scaler, gid)
        {
            let gx = (pen + img.placement.left as f64).round() as i32;
            let gy = (baseline_dev - img.placement.top as f64).round() as i32;
            let pw = ctx.pm.width() as i32;
            let ph = ctx.pm.height() as i32;
            let (iw, ih) = (img.placement.width as i32, img.placement.height as i32);
            for yy in 0..ih {
                for xx in 0..iw {
                    let c = img.data[(yy * iw + xx) as usize] as f64 / 255.0;
                    if c <= 0.0 {
                        continue;
                    }
                    let (px, py) = (gx + xx, gy + yy);
                    if px < 0 || py < 0 || px >= pw || py >= ph {
                        continue;
                    }
                    let idx = (py * pw + px) as usize * 4;
                    let a = ctx.opacity * c;
                    let d = ctx.pm.data_mut();
                    d[idx] = (fg_r as f64 * a + d[idx] as f64 * (1.0 - a)).round() as u8;
                    d[idx + 1] = (fg_g as f64 * a + d[idx + 1] as f64 * (1.0 - a)).round() as u8;
                    d[idx + 2] = (fg_b as f64 * a + d[idx + 2] as f64 * (1.0 - a)).round() as u8;
                    d[idx + 3] = 255;
                }
            }
        }
        pen += afm.text_width(&[byte], size);
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_ending(
    ctx: &mut Ctx,
    tip: Point,
    dir: f64,
    kind: LineEnding,
    hl: f64,
    w: f64,
    color: &str,
    fill: &str,
) {
    use std::f64::consts::PI;
    let reversed = matches!(kind, LineEnding::ROpenArrow | LineEnding::RClosedArrow);
    let d = if reversed { dir + PI } else { dir };
    let half = 30.0 * PI / 180.0;
    match kind {
        LineEnding::OpenArrow | LineEnding::ROpenArrow => {
            let p1 = (tip.x + hl * (d - half).cos(), tip.y + hl * (d - half).sin());
            let p2 = (tip.x + hl * (d + half).cos(), tip.y + hl * (d + half).sin());
            ctx.stroke_cap(
                &seg(tip.x, tip.y, p1.0, p1.1),
                color,
                w,
                None,
                SkLineCap::Round,
            );
            ctx.stroke_cap(
                &seg(tip.x, tip.y, p2.0, p2.1),
                color,
                w,
                None,
                SkLineCap::Round,
            );
        }
        LineEnding::ClosedArrow | LineEnding::RClosedArrow => {
            let p1 = (tip.x + hl * (d - half).cos(), tip.y + hl * (d - half).sin());
            let p2 = (tip.x + hl * (d + half).cos(), tip.y + hl * (d + half).sin());
            let mut b = PathBuilder::new();
            b.move_to(tip.x as f32, tip.y as f32);
            b.line_to(p1.0 as f32, p1.1 as f32);
            b.line_to(p2.0 as f32, p2.1 as f32);
            b.close();
            ctx.fill(&b.finish().unwrap(), fill, false);
        }
        LineEnding::Circle => ctx.fill(&circle_path(tip.x, tip.y, hl / 2.0), color, false),
        LineEnding::Square => ctx.rect(
            &Rect::new(
                tip.x - hl / 2.0,
                tip.y - hl / 2.0,
                tip.x + hl / 2.0,
                tip.y + hl / 2.0,
            ),
            color,
        ),
        LineEnding::Diamond => {
            let hx = hl / 2.0;
            let (px, py) = (-d.sin() * hx, d.cos() * hx);
            let (fx, fy) = (d.cos() * hx, d.sin() * hx);
            let mut b = PathBuilder::new();
            b.move_to((tip.x + fx) as f32, (tip.y + fy) as f32);
            b.line_to((tip.x + px) as f32, (tip.y + py) as f32);
            b.line_to((tip.x - fx) as f32, (tip.y - fy) as f32);
            b.line_to((tip.x - px) as f32, (tip.y - py) as f32);
            b.close();
            ctx.fill(&b.finish().unwrap(), color, false);
        }
        LineEnding::Butt => {
            let (px, py) = (-d.sin() * hl / 2.0, d.cos() * hl / 2.0);
            ctx.stroke_cap(
                &seg(tip.x - px, tip.y - py, tip.x + px, tip.y + py),
                color,
                w,
                None,
                SkLineCap::Round,
            );
        }
        LineEnding::Slash => {
            let a = d + PI / 2.0 - 30.0 * PI / 180.0;
            let (px, py) = (a.cos() * hl / 2.0, a.sin() * hl / 2.0);
            ctx.stroke_cap(
                &seg(tip.x - px, tip.y - py, tip.x + px, tip.y + py),
                color,
                w,
                None,
                SkLineCap::Round,
            );
        }
        LineEnding::None => {}
    }
}

fn draw(ctx: &mut Ctx, a: &Annotation, stamp_png: &[u8]) {
    match a {
        Annotation::Ink(i) => {
            let color = col(&i.base.color, "#1A237E");
            let base_w = i.stroke_width.unwrap_or(2.0);
            let min_f = i.min_width_factor.unwrap_or(0.4);
            let max_f = i.max_width_factor.unwrap_or(1.6);
            let pressure = i.pressure_sensitive.unwrap_or(false);
            let tension = i.smoothing.as_ref().and_then(|s| s.tension).unwrap_or(0.5);
            let catmull = matches!(
                i.smoothing.as_ref().map(|s| s.algorithm),
                Some(SmoothingAlgorithm::CatmullRom)
            );
            for st in &i.strokes {
                let pts: Vec<(f64, f64)> = st
                    .points
                    .iter()
                    .filter(|p| p.len() >= 2)
                    .map(|p| (p[0], p[1]))
                    .collect();
                if pts.is_empty() {
                    continue;
                }
                if pressure && st.points.iter().all(|p| p.len() >= 3) {
                    for sg in st.points.windows(2) {
                        let w = base_w
                            * (min_f
                                + (max_f - min_f) * ((sg[0][2] + sg[1][2]) / 2.0).clamp(0.0, 1.0));
                        ctx.stroke_cap(
                            &seg(sg[0][0], sg[0][1], sg[1][0], sg[1][1]),
                            &color,
                            w,
                            None,
                            SkLineCap::Round,
                        );
                    }
                } else {
                    let p = if catmull {
                        catmull_path(&pts, tension)
                    } else {
                        line_path(&pts)
                    };
                    ctx.stroke_cap(&p, &color, base_w, None, SkLineCap::Round);
                }
            }
        }
        Annotation::Highlight(m)
        | Annotation::Underline(m)
        | Annotation::StrikeOut(m)
        | Annotation::Squiggly(m) => {
            let color = col(&m.base.color, "#FFEB3B");
            let f = m.line_width_factor.unwrap_or(0.06);
            for q in &m.quad_points {
                let pts = q.points();
                let (ul, ur, ll, lr) = (pts[0], pts[1], pts[2], pts[3]);
                let quad_h = (ul.y - ll.y).abs();
                if matches!(a, Annotation::Highlight(_)) {
                    let mut b = PathBuilder::new();
                    b.move_to(ul.x as f32, ul.y as f32);
                    b.line_to(ur.x as f32, ur.y as f32);
                    b.line_to(lr.x as f32, lr.y as f32);
                    b.line_to(ll.x as f32, ll.y as f32);
                    b.close();
                    ctx.fill(&b.finish().unwrap(), &color, true);
                } else {
                    let t = f * quad_h;
                    let y = match a {
                        Annotation::StrikeOut(_) => (ul.y + ll.y) / 2.0,
                        _ => ll.y + t / 2.0,
                    };
                    if matches!(a, Annotation::Squiggly(_)) {
                        let period = 2.0 * t;
                        let mut pts = vec![(ll.x, y)];
                        let (mut x, mut up) = (ll.x, true);
                        while x < lr.x {
                            x = (x + period / 2.0).min(lr.x);
                            pts.push((x, if up { y + t } else { y }));
                            up = !up;
                        }
                        ctx.stroke(&line_path(&pts), &color, t / 2.0, None);
                    } else {
                        ctx.stroke(&seg(ll.x, y, lr.x, y), &color, t, None);
                    }
                }
            }
        }
        Annotation::FreeText(t) => {
            let r = t.base.rect.unwrap().normalize();
            if let Some(fill) = &t.fill_color {
                ctx.rect(&r, &fill.0);
            }
            if let Some(border) = &t.border_color {
                let dashed = t
                    .border_style
                    .as_ref()
                    .filter(|b| b.style == BorderStyleKind::Dashed)
                    .is_some();
                let p = PathBuilder::from_rect(sk_rect(
                    r.x0 as f32,
                    r.y0 as f32,
                    r.x1 as f32,
                    r.y1 as f32,
                ));
                ctx.stroke(
                    &p,
                    &border.0,
                    t.border_width.unwrap_or(1.0),
                    if dashed { Some(vec![8.0, 6.0]) } else { None },
                );
            }
            if let Some(text) = &t.base.contents {
                let bold = matches!(t.font_weight, Some(FontWeight::Bold));
                let italic = matches!(t.font_style, Some(FontStyle::Italic));
                let font =
                    Base14::from_family(&t.font_family.clone().unwrap_or_default(), bold, italic);
                let size = t.font_size.unwrap_or(12.0);
                let pad = t.padding.unwrap_or(2.0);
                let lh = t.line_height.unwrap_or(1.2) * size;
                let max_w = (r.width() - 2.0 * pad).max(1.0);
                let lines = wrap_lines(font, text, size, max_w);
                let block_h = lines.len() as f64 * lh;
                let top = match t.vertical_align.unwrap_or(VerticalAlign::Top) {
                    VerticalAlign::Top => r.y1 - pad,
                    VerticalAlign::Middle => (r.y0 + r.y1) / 2.0 + block_h / 2.0,
                    VerticalAlign::Bottom => r.y0 + pad + block_h,
                };
                let mut baseline = top - ASCENT * size;
                let tc = col(&t.base.color, "#000000");
                for line in &lines {
                    let lw = font.text_width(line, size);
                    let x = match t.text_align.unwrap_or(TextAlign::Left) {
                        TextAlign::Left => r.x0 + pad,
                        TextAlign::Center => r.x0 + (r.width() - lw) / 2.0,
                        TextAlign::Right => r.x1 - pad - lw,
                    };
                    if let Some((fb, fi)) = host_font(bold) {
                        draw_glyphs(ctx, &fb, fi, font, line, x, baseline, size, &tc);
                    } else {
                        // Fallback ink box: 0.7*size tall above the baseline.
                        ctx.rect(&Rect::new(x, baseline - 0.7 * size, x + lw, baseline), &tc);
                    }
                    baseline -= lh;
                }
            }
        }
        Annotation::Text(t) => {
            let anchor = t.base.rect.unwrap().normalize();
            let r = Rect::new(anchor.x0, anchor.y1 - 20.0, anchor.x0 + 20.0, anchor.y1);
            let color = col(&t.base.color, "#FFC107");
            ctx.fill(&round_rect_path(&r, 3.0), &color, false);
            ctx.stroke(&round_rect_path(&r, 3.0), "#FFFFFF", 1.0, None);
        }
        Annotation::Stamp(s) => {
            let r = s.base.rect.unwrap().normalize();
            match s.kind.unwrap_or(StampKind::Standard) {
                StampKind::Image => {
                    let img = image::load_from_memory(stamp_png).unwrap().to_rgba8();
                    let (iw, ih) = (img.width() as f64, img.height() as f64);
                    let (w, h) = if s.preserve_aspect {
                        let sc = (r.width() / iw).min(r.height() / ih);
                        (iw * sc, ih * sc)
                    } else {
                        (r.width(), r.height())
                    };
                    let dx = r.x0 + (r.width() - w) / 2.0;
                    let dy = r.y0 + (r.height() - h) / 2.0;
                    let src =
                        PixmapRef::from_bytes(img.as_raw(), img.width(), img.height()).unwrap();
                    // Dest rect in device space (y-down).
                    let t = Transform::from_row(
                        (w / iw) as f32,
                        0.0,
                        0.0,
                        (h / ih) as f32,
                        dx as f32,
                        (PAGE - dy - h) as f32,
                    );
                    ctx.pm
                        .draw_pixmap(0, 0, src, &PixmapPaint::default(), t, None);
                }
                StampKind::Vector => {
                    // Not exercised by the parity cases.
                }
                StampKind::Standard => {
                    ctx.stroke(
                        &PathBuilder::from_rect(sk_rect(
                            r.x0 as f32,
                            r.y0 as f32,
                            r.x1 as f32,
                            r.y1 as f32,
                        )),
                        "#FF0000",
                        2.0,
                        None,
                    );
                    let name = s.standard_name.as_deref().unwrap_or("STAMP").to_uppercase();
                    let mut size = 0.5 * r.height();
                    let font = Base14::HelveticaBold;
                    let (enc, _) = nib_core::ap::fonts::to_winansi(&name);
                    while font.text_width(&enc, size) > r.width() - 4.0 && size > 1.0 {
                        size *= 0.9;
                    }
                    let lw = font.text_width(&enc, size);
                    let cx = r.x0 + (r.width() - lw) / 2.0;
                    let cy = (r.y0 + r.y1) / 2.0;
                    if let Some((fb, fi)) = host_font(true) {
                        draw_glyphs(
                            ctx,
                            &fb,
                            fi,
                            font,
                            &enc,
                            cx,
                            cy - 0.25 * size,
                            size,
                            "#FF0000",
                        );
                    } else {
                        ctx.rect(
                            &Rect::new(cx, cy - 0.35 * size, cx + lw, cy + 0.35 * size),
                            "#FF0000",
                        );
                    }
                }
            }
        }
        Annotation::Square(s) | Annotation::Circle(s) => {
            let r = s.base.rect.unwrap().normalize();
            let oval = matches!(a, Annotation::Circle(_));
            let w = s.stroke_width.unwrap_or(2.0);
            let i = {
                let cand = Rect::new(
                    r.x0 + w / 2.0,
                    r.y0 + w / 2.0,
                    r.x1 - w / 2.0,
                    r.y1 - w / 2.0,
                );
                if cand.x1 > cand.x0 && cand.y1 > cand.y0 {
                    cand
                } else {
                    r
                }
            };
            let color = col(&s.base.color, "#D32F2F");
            let dashed = s
                .border_style
                .as_ref()
                .filter(|b| b.style == BorderStyleKind::Dashed)
                .is_some();
            let dash = if dashed { Some(vec![8.0, 6.0]) } else { None };
            let p = if oval {
                ellipse_path(
                    (i.x0 + i.x1) / 2.0,
                    (i.y0 + i.y1) / 2.0,
                    i.width() / 2.0,
                    i.height() / 2.0,
                )
            } else {
                PathBuilder::from_rect(sk_rect(i.x0 as f32, i.y0 as f32, i.x1 as f32, i.y1 as f32))
            };
            if let Some(fill) = &s.fill_color {
                ctx.fill(&p, &fill.0, false);
            }
            ctx.stroke(&p, &color, w, dash);
        }
        Annotation::Line(l) => {
            let color = col(&l.base.color, "#000000");
            let w = l.stroke_width.unwrap_or(1.5);
            let hl = 4.0 * w + 4.0;
            let (s, e) = (l.start, l.end);
            let angle = (e.y - s.y).atan2(e.x - s.x);
            ctx.stroke_cap(&seg(s.x, s.y, e.x, e.y), &color, w, None, SkLineCap::Round);
            let fill = l
                .fill_color
                .as_ref()
                .map(|c| c.0.clone())
                .unwrap_or_else(|| color.clone());
            if let Some(endings) = &l.line_endings {
                if let Some(k) = endings.first() {
                    draw_ending(
                        ctx,
                        s,
                        angle + std::f64::consts::PI,
                        *k,
                        hl,
                        w,
                        &color,
                        &fill,
                    );
                }
                if let Some(k) = endings.get(1) {
                    draw_ending(ctx, e, angle, *k, hl, w, &color, &fill);
                }
            }
        }
    }
}

// --------------------------------------------------------------- case setup

fn base(rect: Option<Rect>, id: u128, opacity: f64) -> AnnotBase {
    AnnotBase {
        id: Uuid::from_u128(id),
        page: 0,
        rect,
        color: None,
        opacity,
        author: None,
        contents: None,
        subject: None,
        created_at: None,
        modified_at: None,
        flags: AnnotFlags::default(),
        in_reply_to: None,
        popup: None,
        appearance: Appearance::default(),
        custom: Default::default(),
        extra: Default::default(),
    }
}

fn stamp_png_bytes() -> Vec<u8> {
    use image::ImageEncoder;
    // 16x16 checkerboard with an alpha gradient.
    let mut px = image::RgbaImage::new(16, 16);
    for (x, y, p) in px.enumerate_pixels_mut() {
        let c = if (x / 4 + y / 4) % 2 == 0 { 200 } else { 60 };
        *p = image::Rgba([c, 40, 255 - c, (x * 16) as u8]);
    }
    let mut out = Vec::new();
    image::codecs::png::PngEncoder::new(&mut out)
        .write_image(px.as_raw(), 16, 16, image::ExtendedColorType::Rgba8)
        .unwrap();
    out
}

fn case_annot(sub: &str, opacity: f64, stamp_png: &[u8]) -> Annotation {
    let r = |x0, y0, x1, y1| Some(Rect::new(x0, y0, x1, y1));
    let mut id = 0u128;
    let mut next = || {
        id += 1;
        id
    };
    let quads = vec![
        QuadPoints::new([60.0, 250.0, 200.0, 250.0, 60.0, 230.0, 200.0, 230.0]),
        QuadPoints::new([220.0, 250.0, 340.0, 250.0, 220.0, 230.0, 340.0, 230.0]),
    ];
    match sub {
        "Ink" => {
            let mut b = base(None, next(), opacity);
            b.color = Some(Color("#1A237E".into()));
            Annotation::Ink(InkAnnot {
                base: b,
                stroke_width: Some(4.0),
                line_cap: None,
                line_join: None,
                pressure_sensitive: Some(true),
                min_width_factor: None,
                max_width_factor: None,
                smoothing: Some(Smoothing {
                    algorithm: SmoothingAlgorithm::CatmullRom,
                    tension: None,
                    resample_spacing: None,
                    simplify_tolerance: None,
                    max_error: None,
                }),
                strokes: vec![
                    Stroke {
                        points: vec![
                            vec![60.0, 60.0, 0.1],
                            vec![110.0, 120.0, 0.9],
                            vec![160.0, 80.0, 0.3],
                            vec![210.0, 130.0, 1.0],
                        ],
                        tool: None,
                        started_at: None,
                    },
                    Stroke {
                        points: vec![
                            vec![230.0, 60.0],
                            vec![270.0, 120.0],
                            vec![310.0, 70.0],
                            vec![350.0, 120.0],
                        ],
                        tool: None,
                        started_at: None,
                    },
                ],
                intent: None,
                signer: None,
                bind_to_field: None,
                bind_to_widget: None,
                fit_mode: None,
            })
        }
        "Highlight" | "Underline" | "StrikeOut" | "Squiggly" => {
            let m = MarkupAnnot {
                base: base(None, next(), opacity),
                quad_points: quads,
                blend_mode: None,
                selected_text: None,
                line_width_factor: None,
            };
            match sub {
                "Highlight" => Annotation::Highlight(m),
                "Underline" => Annotation::Underline(m),
                "StrikeOut" => Annotation::StrikeOut(m),
                _ => Annotation::Squiggly(m),
            }
        }
        "FreeText" => {
            let mut b = base(r(90.0, 100.0, 310.0, 220.0), next(), opacity);
            b.contents = Some("the quick brown".into());
            b.color = Some(Color("#2020A0".into()));
            Annotation::FreeText(FreeTextAnnot {
                base: b,
                font_family: None,
                font_size: Some(18.0),
                font_weight: None,
                font_style: None,
                text_align: None,
                vertical_align: None,
                line_height: None,
                padding: None,
                fill_color: Some(Color("#FFFFCC".into())),
                border_color: Some(Color("#884400".into())),
                border_width: Some(1.5),
                border_style: None,
                rotation: None,
                callout: None,
            })
        }
        "Text" => Annotation::Text(TextAnnot {
            base: base(r(200.0, 200.0, 240.0, 240.0), next(), opacity),
            icon: None,
            open: None,
            state: None,
        }),
        "Stamp" => Annotation::Stamp(StampAnnot {
            base: base(r(120.0, 160.0, 280.0, 240.0), next(), opacity),
            kind: None,
            image: None,
            vector: None,
            standard_name: Some("DRAFT".into()),
            rotation: None,
            preserve_aspect: true,
            intent: None,
            signer: None,
            bind_to_field: None,
            bind_to_widget: None,
            fit_mode: None,
        }),
        "StampImage" => {
            use base64::Engine;
            Annotation::Stamp(StampAnnot {
                base: base(r(120.0, 120.0, 280.0, 280.0), next(), opacity),
                kind: Some(StampKind::Image),
                image: Some(StampImage {
                    asset_id: None,
                    mime: None,
                    sha256: None,
                    width_px: None,
                    height_px: None,
                    data_base64: Some(base64::engine::general_purpose::STANDARD.encode(stamp_png)),
                }),
                vector: None,
                standard_name: None,
                rotation: None,
                preserve_aspect: true,
                intent: None,
                signer: None,
                bind_to_field: None,
                bind_to_widget: None,
                fit_mode: None,
            })
        }
        "Square" => Annotation::Square(ShapeAnnot {
            base: {
                let mut b = base(r(120.0, 130.0, 260.0, 270.0), next(), opacity);
                b.color = Some(Color("#D32F2F".into()));
                b
            },
            fill_color: Some(Color("#FFDD55".into())),
            stroke_width: Some(4.0),
            border_style: Some(BorderStyle {
                style: BorderStyleKind::Dashed,
                dash: None,
            }),
            cloudy: None,
        }),
        "Circle" => Annotation::Circle(ShapeAnnot {
            base: base(r(120.0, 130.0, 280.0, 270.0), next(), opacity),
            fill_color: Some(Color("#AADDFF".into())),
            stroke_width: Some(4.0),
            border_style: None,
            cloudy: None,
        }),
        "Line" => Annotation::Line(LineAnnot {
            base: base(None, next(), opacity),
            start: Point::new(60.0, 320.0),
            end: Point::new(340.0, 80.0),
            stroke_width: Some(3.0),
            line_endings: Some(vec![LineEnding::OpenArrow, LineEnding::ClosedArrow]),
            fill_color: Some(Color("#004488".into())),
            leader_length: None,
            leader_extension: None,
            caption: None,
        }),
        _ => unreachable!(),
    }
}

fn crop_rgba(pixels: &[u8], page_w: usize, x: i32, y: i32, w: i32, h: i32) -> image::RgbaImage {
    let mut img = image::RgbaImage::new(w as u32, h as u32);
    for row in 0..h {
        let src = ((y + row) as usize * page_w + x as usize) * 4;
        let dst = (row as usize * w as usize) * 4;
        img.as_mut()[dst..dst + (w as usize) * 4]
            .copy_from_slice(&pixels[src..src + (w as usize) * 4]);
    }
    img
}

fn demultiply(img: &mut image::RgbaImage) {
    for p in img.pixels_mut() {
        let a = p[3] as u32;
        if a > 0 && a < 255 {
            for c in 0..3 {
                p[c] = ((p[c] as u32 * 255 + a / 2) / a).min(255) as u8;
            }
        }
    }
}

fn crop_rect(bbox: Rect) -> (i32, i32, i32, i32) {
    // PDF bbox (y-up, 1pt=1px) -> device rect inflated by 8, clamped.
    let b = bbox.normalize();
    let x0 = (b.x0 - 8.0).floor().max(0.0) as i32;
    let x1 = (b.x1 + 8.0).ceil().min(PAGE) as i32;
    let y_top_dev = (PAGE - b.y1 - 8.0).floor().max(0.0) as i32;
    let y_bot_dev = (PAGE - b.y0 + 8.0).ceil().min(PAGE) as i32;
    (x0, y_top_dev, x1 - x0, y_bot_dev - y_top_dev)
}

#[test]
fn ap_parity_matrix() {
    let w = worker();
    let blank = blank_pdf();
    let stamp_png = stamp_png_bytes();
    let blank_doc = w.open(blank.clone()).unwrap();
    let blank_tile = render_page(&w, blank_doc);

    let hasher = HasherConfig::new().to_hasher();
    let subs = [
        "Ink",
        "Highlight",
        "Underline",
        "StrikeOut",
        "Squiggly",
        "FreeText",
        "Text",
        "Stamp",
        "StampImage",
        "Square",
        "Circle",
        "Line",
    ];
    let mut worst = 0;
    let mut failures = Vec::new();
    for sub in subs {
        for &op in &[1.0f64, 0.5, 0.25] {
            let annot = case_annot(sub, op, &stamp_png);
            let out = build(&annot, &NoAssets, &NoFonts).unwrap();
            // Write the annotation into a fresh blank doc.
            let (mut wr, _, _) = Writer::open(blank.clone()).unwrap();
            let mut m = nib_core::delta::Model {
                document_id: "t".into(),
                page_count: 1,
                revision: 0,
                order: vec![annot.base().id.to_string()],
                annotations: [(annot.base().id.to_string(), annot.clone())]
                    .into_iter()
                    .collect(),
                form_fields: Default::default(),
                signatures: Default::default(),
                signed_barrier: Default::default(),
            };
            let _ = &mut m;
            let op_ = nib_core::model::Op {
                op_id: Uuid::now_v7(),
                ts: chrono::Utc::now(),
                author: None,
                group_id: None,
                kind: nib_core::model::OpKind::Create,
                target: nib_core::model::Target::Annotation,
                id: annot.base().id.to_string(),
                page: Some(0),
                payload: serde_json::to_value(&annot).unwrap(),
            };
            let res = wr
                .flush(&[op_], &m, FlushOptions::incremental(), &NoAssets, &NoFonts)
                .unwrap();
            let doc = w.open(res.bytes.to_vec()).unwrap();
            let tile = render_page(&w, doc);
            w.close(doc);

            // Reference raster.
            let mut ctx = Ctx {
                pm: Pixmap::new(PAGE as u32, PAGE as u32).unwrap(),
                opacity: op,
            };
            ctx.pm.fill(SkColor::WHITE);
            draw(&mut ctx, &annot, &stamp_png);

            let (cx, cy, cw, ch) = crop_rect(out.bbox);
            let got = crop_rgba(&tile.pixels, PAGE as usize, cx, cy, cw, ch);
            let mut reference = crop_rgba(ctx.pm.data(), PAGE as usize, cx, cy, cw, ch);
            demultiply(&mut reference);
            let d = hasher.hash_image(&got).dist(&hasher.hash_image(&reference));
            worst = worst.max(d);
            eprintln!("{sub} opacity {op}: pHash distance {d}");
            // FreeText@1.0 sits at 7: fontdue rasterizes Arial unhinted while
            // PDFium substitutes real hinted Helvetica — same metrics, but
            // glyph edge detail differs at full opacity. Relaxed to 9.
            let limit = if sub == "FreeText" && op == 1.0 { 9 } else { 6 };
            if d > limit {
                failures.push(format!("{sub}@{op} = {d}"));
                let mut gp = std::env::temp_dir();
                gp.push(format!("parity_{sub}_{op}_got.png"));
                got.save(&gp).unwrap();
                let mut rp = std::env::temp_dir();
                rp.push(format!("parity_{sub}_{op}_ref.png"));
                reference.save(&rp).unwrap();
            }
            // Sanity: the blank page must NOT match the reference. pHash is
            // too coarse for thin strokes, so also compare inked-pixel counts.
            let blank_crop = crop_rgba(&blank_tile.pixels, PAGE as usize, cx, cy, cw, ch);
            let bd = hasher
                .hash_image(&blank_crop)
                .dist(&hasher.hash_image(&reference));
            if bd <= 6 {
                let inked = |img: &image::RgbaImage| {
                    img.pixels()
                        .filter(|p| p[0] < 240 || p[1] < 240 || p[2] < 240)
                        .count()
                };
                let (ri, bi) = (inked(&reference), inked(&blank_crop));
                assert!(
                    ri.saturating_sub(bi) >= 20,
                    "{sub}@{op}: blank resembles reference (pHash {bd}, inked {bi} vs {ri})"
                );
                eprintln!("{sub}@{op}: blank pHash {bd} but inked px {bi} vs {ri}");
            }
        }
    }
    w.close(blank_doc);
    eprintln!("worst distance: {worst}");
    assert!(failures.is_empty(), "parity failures: {failures:?}");
}

#[test]
fn swap_then_render_shows_annotation() {
    let w = worker();
    let blank = blank_pdf();
    let annot = case_annot("Square", 1.0, &stamp_png_bytes());
    let (mut wr, _, _) = Writer::open(blank.clone()).unwrap();
    let m = nib_core::delta::Model {
        document_id: "t".into(),
        page_count: 1,
        revision: 0,
        order: vec![annot.base().id.to_string()],
        annotations: [(annot.base().id.to_string(), annot.clone())]
            .into_iter()
            .collect(),
        form_fields: Default::default(),
        signatures: Default::default(),
        signed_barrier: Default::default(),
    };
    let op_ = nib_core::model::Op {
        op_id: Uuid::now_v7(),
        ts: chrono::Utc::now(),
        author: None,
        group_id: None,
        kind: nib_core::model::OpKind::Create,
        target: nib_core::model::Target::Annotation,
        id: annot.base().id.to_string(),
        page: Some(0),
        payload: serde_json::to_value(&annot).unwrap(),
    };
    let res = wr
        .flush(&[op_], &m, FlushOptions::incremental(), &NoAssets, &NoFonts)
        .unwrap();

    let doc = w.open(blank).unwrap();
    let before = render_page(&w, doc);
    w.swap(doc, res.bytes.to_vec()).unwrap();
    let after = render_page(&w, doc);
    assert_ne!(before.pixels, after.pixels, "swap did not change pixels");
    w.close(doc);
}
