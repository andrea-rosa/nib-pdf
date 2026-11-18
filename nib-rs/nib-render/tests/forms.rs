//! Form read-back and render-flag tests (M4): field values as pdfium sees
//! them after a writer flush, flatten parity, and the NO_FORMS tile flag.

use std::path::PathBuf;
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use image_hasher::HasherConfig;
use nib_core::ap::{NoAssets, NoFonts};
use nib_core::delta::Engine;
use nib_core::model::{Op, OpKind, Target};
use nib_core::writer::{FlushOptions, Writer};
use nib_render::types::{flags, FieldValue, IRect, ISize, TileResult};
use nib_render::{Priority, RenderWorker, TileKey, TileRequest};
use serde_json::{json, Value};
use uuid::Uuid;

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
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../target/pdfium")
        .join(triple)
}

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../nib-core/tests/fixtures")
}

/// One process-wide worker: `Pdfium::new`/`FPDF_InitLibrary` run once.
fn worker() -> Arc<RenderWorker> {
    static WORKER: OnceLock<Arc<RenderWorker>> = OnceLock::new();
    Arc::clone(WORKER.get_or_init(|| RenderWorker::spawn(Some(host_pdfium_dir()))))
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

const SCALE: f64 = 1.5;
// Fixture page: 612x792 pt.
const PAGE_W: i32 = (612.0 * SCALE) as i32;
const PAGE_H: i32 = (792.0 * SCALE) as i32;

fn render_page(w: &RenderWorker, doc: u64, tile_flags: u8) -> TileResult {
    let req = TileRequest {
        key: TileKey {
            page: 0,
            bucket: 0,
            tx: 0,
            ty: 0,
            revision: 0,
            flags: tile_flags,
        },
        device_rect: IRect {
            x: 0,
            y: 0,
            width: PAGE_W,
            height: PAGE_H,
        },
        page_device_size: ISize {
            width: PAGE_W,
            height: PAGE_H,
        },
        generation: u64::MAX,
        priority: Priority::Visible,
        distance: 0,
    };
    wait_tile(w.render_tile(doc, req))
}

fn fop(name: &str, value: Value) -> Op {
    Op {
        op_id: Uuid::now_v7(),
        ts: chrono::Utc::now(),
        author: None,
        group_id: None,
        kind: OpKind::Update,
        target: Target::FormField,
        id: name.to_string(),
        page: Some(0),
        payload: json!({ "value": value }),
    }
}

struct FormSetup {
    doc: u64,
    writer: Writer,
}

/// Open form.pdf on the worker, flush the standard values, swap.
fn setup_with_values() -> FormSetup {
    let bytes = std::fs::read(fixtures_dir().join("form.pdf")).unwrap();
    let doc = worker().open(bytes.clone()).unwrap();
    let (mut writer, _annots, fields) = Writer::open(bytes).unwrap();
    let mut e = Engine::with_base("doc".into(), writer.page_count(), vec![], fields);
    let ops: Vec<Op> = [
        fop("name", json!("Andrea Rosa")),
        fop("notes", json!("line one\nline two")),
        fop("code", json!("123456")),
        fop("secret", json!("hunter2")),
        fop("agree", json!("Yes")),
        fop("newsletter", json!("On")),
        fop("color", json!("Green")),
        fop("country", json!("IT")),
        fop("city", json!("Turin")),
        fop("plan", json!(["Pro", "Enterprise"])),
        fop("bottom.note", json!("bottom text")),
    ]
    .into();
    e.apply(ops).unwrap();
    let ops = e.flush_ops();
    let out = writer
        .flush(
            &ops,
            e.model(),
            FlushOptions::incremental(),
            &NoAssets,
            &NoFonts,
        )
        .unwrap();
    worker().swap(doc, out.bytes.to_vec()).unwrap();
    FormSetup { doc, writer }
}

fn by_name<'a>(vals: &'a [FieldValue], name: &str) -> Vec<&'a FieldValue> {
    vals.iter().filter(|v| v.name == name).collect()
}

#[test]
fn form_values_readback_via_pdfium() {
    let s = setup_with_values();
    let vals = worker().form_field_values(s.doc).unwrap();

    for (name, expect) in [
        ("name", "Andrea Rosa"),
        ("notes", "line one\nline two"),
        ("code", "123456"),
        ("secret", "hunter2"),
        ("bottom.note", "bottom text"),
    ] {
        let v = by_name(&vals, name);
        assert_eq!(v.len(), 1, "{name}: {vals:?}");
        assert_eq!(v[0].value.as_deref(), Some(expect), "{name}");
    }

    let agree = by_name(&vals, "agree");
    assert_eq!(agree.len(), 1);
    assert_eq!(agree[0].checked, Some(true));
    assert_eq!(agree[0].value.as_deref(), Some("Yes"));

    let news = by_name(&vals, "newsletter");
    assert_eq!(news.len(), 1);
    assert_eq!(news[0].checked, Some(true));
    assert_eq!(news[0].value.as_deref(), Some("On"));

    let color = by_name(&vals, "color");
    assert_eq!(color.len(), 3, "radio kids: {color:?}");
    let checked: Vec<_> = color.iter().filter(|v| v.checked == Some(true)).collect();
    assert_eq!(checked.len(), 1, "color checked: {color:?}");
    assert_eq!(checked[0].value.as_deref(), Some("Green"));

    // pdfium reports the selected option's label, not its export value.
    assert_eq!(by_name(&vals, "country")[0].value.as_deref(), Some("Italy"));
    // pdfium-render has no accessor for an editable combo's free text
    // ("Turin" is not in /Opt) → read back as None.
    assert_eq!(by_name(&vals, "city")[0].value, None);
    let plan = by_name(&vals, "plan")[0].value.clone().unwrap_or_default();
    assert!(plan.contains("Pro"), "plan value {plan:?}");

    worker().close(s.doc);
}

fn phash(t: &TileResult) -> image_hasher::ImageHash {
    let img = image::RgbaImage::from_raw(t.width, t.height, t.pixels.clone()).unwrap();
    HasherConfig::new().to_hasher().hash_image(&img)
}

#[test]
fn flatten_parity() {
    let s = setup_with_values();
    let mut writer = s.writer;
    let e = Engine::with_base("doc".into(), writer.page_count(), vec![], Vec::new());
    // Flatten the already-flushed revision: same model content, no new ops.
    let out = writer
        .flush(
            &[],
            e.model(),
            FlushOptions::incremental().flatten(true),
            &NoAssets,
            &NoFonts,
        )
        .unwrap();
    let w = worker();
    // (a) flushed-with-values revision is already on the worker.
    let a = render_page(&w, s.doc, 0);
    w.swap(s.doc, out.bytes.to_vec()).unwrap();
    let b = render_page(&w, s.doc, 0);
    let d_ab = phash(&a).dist(&phash(&b));
    eprintln!("flatten parity: pHash(a,b) = {d_ab}");
    assert!(d_ab <= 4, "flatten parity pHash {d_ab} > 4");

    // Sanity: values-flushed vs untouched fixture must differ.
    let orig = std::fs::read(fixtures_dir().join("form.pdf")).unwrap();
    let doc2 = w.open(orig).unwrap();
    let c = render_page(&w, doc2, 0);
    let d_ac = phash(&a).dist(&phash(&c));
    eprintln!("values vs untouched: pHash = {d_ac}");
    assert!(d_ac > 4, "expected pHash > 4, got {d_ac}");
    w.close(doc2);
    w.close(s.doc);
}

#[test]
fn no_forms_flag_hides_widgets() {
    let s = setup_with_values();
    let w = worker();
    let with_forms = render_page(&w, s.doc, 0);
    let no_forms = render_page(&w, s.doc, flags::NO_FORMS);

    // `name` widget rect [150 700 400 722]pt → device px at SCALE, y flipped.
    let mut diff_inside = 0usize;
    let x0 = (150.0 * SCALE) as usize;
    let x1 = (400.0 * SCALE) as usize;
    let y0 = ((792.0 - 722.0) * SCALE) as usize;
    let y1 = ((792.0 - 700.0) * SCALE) as usize;
    for y in y0..y1 {
        for x in x0..x1 {
            let i = (y * PAGE_W as usize + x) * 4;
            if with_forms.pixels[i..i + 4] != no_forms.pixels[i..i + 4] {
                diff_inside += 1;
            }
        }
    }
    assert!(
        diff_inside > 0,
        "expected differing pixels inside the name widget rect"
    );

    // A region outside all widgets (top-left margin) stays identical.
    let mut diff_outside = 0usize;
    let (ox0, ox1) = (30usize, 120usize);
    let (oy0, oy1) = (30usize, 60usize);
    for y in oy0..oy1 {
        for x in ox0..ox1 {
            let i = (y * PAGE_W as usize + x) * 4;
            if with_forms.pixels[i..i + 4] != no_forms.pixels[i..i + 4] {
                diff_outside += 1;
            }
        }
    }
    assert_eq!(diff_outside, 0);
    w.close(s.doc);
}
