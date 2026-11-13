//! Deterministic PDF fixture generator + integrity checks.
//!
//! Regenerate with:
//!   cargo test -p nib-core --test gen_fixtures regenerate_fixtures -- --ignored
//! The files are committed under `tests/fixtures/`; the non-ignored test only
//! verifies they exist, parse, and stay small.

use lopdf::content::{Content, Operation};
use lopdf::{dictionary, Document, Object, ObjectId, Stream};
use std::path::{Path, PathBuf};

fn fixtures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

const FIXTURES: &[&str] = &[
    "text.pdf",
    "vector.pdf",
    "image.pdf",
    "rotated90.pdf",
    "cropbox.pdf",
    "text-100.pdf",
];

/// Every committed fixture exists, parses with lopdf, and stays < 200 KB.
#[test]
fn fixtures_are_valid() {
    for name in FIXTURES {
        let path = fixtures_dir().join(name);
        let bytes = std::fs::read(&path).unwrap_or_else(|_| panic!("missing {name}"));
        assert!(
            bytes.len() < 200 * 1024,
            "{name} too large: {}",
            bytes.len()
        );
        Document::load_mem(&bytes).unwrap_or_else(|e| panic!("{name} unparseable: {e}"));
    }
    let pages = Document::load(fixtures_dir().join("text-100.pdf"))
        .unwrap()
        .get_pages()
        .len();
    assert_eq!(pages, 100);
}

// ---------- generator ----------

struct Builder {
    doc: Document,
    pages_id: Option<ObjectId>,
    page_ids: Vec<ObjectId>,
}

impl Builder {
    fn new() -> Self {
        Builder {
            doc: Document::new(),
            pages_id: None,
            page_ids: Vec::new(),
        }
    }

    fn add(&mut self, obj: impl Into<Object>) -> ObjectId {
        self.doc.add_object(obj.into())
    }

    fn font_helvetica(&mut self) -> ObjectId {
        self.add(dictionary! {
            "Type" => "Font",
            "Subtype" => "Type1",
            "BaseFont" => "Helvetica",
        })
    }

    fn stream(&mut self, ops: Vec<Operation>) -> ObjectId {
        let bytes = Content { operations: ops }.encode().unwrap();
        self.add(Stream::new(dictionary! {}, bytes))
    }

    /// Adds a /Page under the shared /Pages tree.
    fn add_page(
        &mut self,
        media_box: [f64; 4],
        crop_box: Option<[f64; 4]>,
        rotate: Option<i64>,
        contents: ObjectId,
        resources: ObjectId,
    ) {
        let pages = *self
            .pages_id
            .get_or_insert_with(|| self.doc.new_object_id());
        let mut page = dictionary! {
            "Type" => "Page",
            "Parent" => Object::Reference(pages),
            "MediaBox" => box4(media_box),
            "Contents" => Object::Reference(contents),
            "Resources" => Object::Reference(resources),
        };
        if let Some(c) = crop_box {
            page.set("CropBox", box4(c));
        }
        if let Some(r) = rotate {
            page.set("Rotate", r);
        }
        self.page_ids
            .push(self.doc.add_object(Object::Dictionary(page)));
    }

    fn finish(mut self) -> Document {
        let pages = self.pages_id.expect("no pages added");
        let kids: Vec<Object> = self
            .page_ids
            .iter()
            .map(|id| Object::Reference(*id))
            .collect();
        self.doc.objects.insert(
            pages,
            Object::Dictionary(dictionary! {
                "Type" => "Pages",
                "Kids" => kids.clone(),
                "Count" => kids.len() as i64,
            }),
        );
        let catalog = self.add(dictionary! {
            "Type" => "Catalog",
            "Pages" => Object::Reference(pages),
        });
        self.doc.trailer.set("Root", Object::Reference(catalog));
        self.doc
    }
}

fn box4(v: [f64; 4]) -> Vec<Object> {
    v.iter().map(|f| Object::from(*f)).collect()
}

fn ops(ops: &[(&str, Vec<Object>)]) -> Vec<Operation> {
    ops.iter()
        .map(|(op, operands)| Operation::new(op, operands.clone()))
        .collect()
}

/// Text-showing helper: `BT /F1 <size> Tf <x> <y> Td (<s>) Tj ET`.
fn text_line(x: f64, y: f64, size: f64, s: &str) -> Vec<Operation> {
    ops(&[
        ("BT", vec![]),
        ("Tf", vec!["F1".into(), size.into()]),
        ("Td", vec![x.into(), y.into()]),
        ("Tj", vec![Object::string_literal(s)]),
        ("ET", vec![]),
    ])
}

fn text_pdf(b: &mut Builder) {
    let font = b.font_helvetica();
    let resources = b.add(dictionary! {
        "Font" => dictionary! { "F1" => Object::Reference(font) },
    });
    let mut content = Vec::new();
    for (i, line) in [
        "The quick brown fox jumps over the lazy dog.",
        "Pack my box with five dozen liquor jugs.",
        "Sphinx of black quartz, judge my vow.",
        "nib fixture: text.pdf",
    ]
    .iter()
    .enumerate()
    {
        content.extend(text_line(40.0, 160.0 - i as f64 * 16.0, 12.0, line));
    }
    let contents = b.stream(content);
    b.add_page([0.0, 0.0, 200.0, 200.0], None, None, contents, resources);
}

fn vector_pdf(b: &mut Builder) {
    let resources = b.add(dictionary! {});
    let mut content = Vec::new();
    // Filled triangle, stroked rect, stroked line — distinct colors.
    content.extend(ops(&[
        ("rg", vec![0.8.into(), 0.1.into(), 0.1.into()]),
        ("m", vec![50.0.into(), 40.0.into()]),
        ("l", vec![100.0.into(), 150.0.into()]),
        ("l", vec![150.0.into(), 40.0.into()]),
        ("h", vec![]),
        ("f", vec![]),
        ("RG", vec![0.1.into(), 0.1.into(), 0.8.into()]),
        ("w", vec![3.0.into()]),
        (
            "re",
            vec![20.0.into(), 20.0.into(), 160.0.into(), 30.0.into()],
        ),
        ("S", vec![]),
        ("RG", vec![0.0.into(), 0.5.into(), 0.0.into()]),
        ("m", vec![20.0.into(), 190.0.into()]),
        ("l", vec![180.0.into(), 170.0.into()]),
        ("S", vec![]),
    ]));
    let contents = b.stream(content);
    b.add_page([0.0, 0.0, 200.0, 200.0], None, None, contents, resources);
}

fn image_pdf(b: &mut Builder) {
    // 64×64 uncompressed DeviceRGB gradient.
    let mut samples = Vec::with_capacity(64 * 64 * 3);
    for y in 0..64 {
        for x in 0..64 {
            samples.push((x * 4) as u8);
            samples.push((y * 4) as u8);
            samples.push(255 - ((x + y) * 2) as u8);
        }
    }
    let image = b.add(
        Stream::new(
            dictionary! {
                "Type" => "XObject",
                "Subtype" => "Image",
                "Width" => 64,
                "Height" => 64,
                "ColorSpace" => "DeviceRGB",
                "BitsPerComponent" => 8,
            },
            samples,
        )
        .with_compression(false),
    );
    let resources = b.add(dictionary! {
        "XObject" => dictionary! { "Im1" => Object::Reference(image) },
    });
    // Draw the 64×64 image scaled to 300×300 at (50,50) on a 400×400 page.
    let contents = b.stream(ops(&[
        ("q", vec![]),
        (
            "cm",
            vec![
                300.0.into(),
                0.0.into(),
                0.0.into(),
                300.0.into(),
                50.0.into(),
                50.0.into(),
            ],
        ),
        ("Do", vec!["Im1".into()]),
        ("Q", vec![]),
    ]));
    b.add_page([0.0, 0.0, 400.0, 400.0], None, None, contents, resources);
}

fn rotated90_pdf(b: &mut Builder) {
    let font = b.font_helvetica();
    let resources = b.add(dictionary! {
        "Font" => dictionary! { "F1" => Object::Reference(font) },
    });
    // Asymmetric mark: a bar along the left edge plus text in one corner.
    let mut content = ops(&[
        ("rg", vec![0.9.into(), 0.2.into(), 0.2.into()]),
        (
            "re",
            vec![0.0.into(), 0.0.into(), 20.0.into(), 300.0.into()],
        ),
        ("f", vec![]),
    ]);
    content.extend(text_line(30.0, 280.0, 14.0, "ROTATED 90 LEFT-EDGE MARK"));
    let contents = b.stream(content);
    b.add_page(
        [0.0, 0.0, 200.0, 300.0],
        None,
        Some(90),
        contents,
        resources,
    );
}

fn cropbox_pdf(b: &mut Builder) {
    let font = b.font_helvetica();
    let resources = b.add(dictionary! {
        "Font" => dictionary! { "F1" => Object::Reference(font) },
    });
    // MediaBox 700×900, CropBox [100 150 500 750]; content inside the crop.
    let mut content = ops(&[
        ("rg", vec![0.2.into(), 0.4.into(), 0.7.into()]),
        (
            "re",
            vec![150.0.into(), 200.0.into(), 300.0.into(), 500.0.into()],
        ),
        ("f", vec![]),
    ]);
    content.extend(text_line(120.0, 700.0, 20.0, "CROPBOX INSIDE MEDIABOX"));
    let contents = b.stream(content);
    b.add_page(
        [0.0, 0.0, 700.0, 900.0],
        Some([100.0, 150.0, 500.0, 750.0]),
        None,
        contents,
        resources,
    );
}

fn text_100_pdf(b: &mut Builder) {
    let font = b.font_helvetica();
    let resources = b.add(dictionary! {
        "Font" => dictionary! { "F1" => Object::Reference(font) },
    });
    for n in 1..=100 {
        let mut content = text_line(50.0, 700.0, 24.0, &format!("Page {n}"));
        for i in 0..4 {
            content.extend(text_line(
                50.0,
                650.0 - i as f64 * 18.0,
                11.0,
                &format!("Lorem ipsum dolor sit amet, line {i} of page {n}."),
            ));
        }
        let contents = b.stream(content);
        b.add_page([0.0, 0.0, 400.0, 800.0], None, None, contents, resources);
    }
}

/// Writes all fixtures deterministically (no /ID, no dates).
#[test]
#[ignore]
fn regenerate_fixtures() {
    let dir = fixtures_dir();
    std::fs::create_dir_all(&dir).unwrap();

    let mut b = Builder::new();
    text_pdf(&mut b);
    b.finish().save(dir.join("text.pdf")).unwrap();

    let mut b = Builder::new();
    vector_pdf(&mut b);
    b.finish().save(dir.join("vector.pdf")).unwrap();

    let mut b = Builder::new();
    image_pdf(&mut b);
    b.finish().save(dir.join("image.pdf")).unwrap();

    let mut b = Builder::new();
    rotated90_pdf(&mut b);
    b.finish().save(dir.join("rotated90.pdf")).unwrap();

    let mut b = Builder::new();
    cropbox_pdf(&mut b);
    b.finish().save(dir.join("cropbox.pdf")).unwrap();

    let mut b = Builder::new();
    text_100_pdf(&mut b);
    b.finish().save(dir.join("text-100.pdf")).unwrap();
}
