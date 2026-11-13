//! Deterministic PDF fixture generator + integrity checks.
//!
//! Regenerate with:
//!   cargo test -p nib-core --test gen_fixtures regenerate_fixtures -- --ignored
//! The files are committed under `tests/fixtures/`; the non-ignored test only
//! verifies they exist, parse, and stay small.

use lopdf::content::{Content, Operation};
use lopdf::xref::XrefType;
use lopdf::{dictionary, Document, Object, ObjectId, SaveOptions, Stream, StringFormat};
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
    "incr-classic.pdf",
    "incr-xrefstream.pdf",
    "incr-linearized.pdf",
    "incr-presigned.pdf",
    "incr-hybrid.pdf",
    "annots-existing.pdf",
    "stamp-src.png",
    "stamp-src.jpg",
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
        if name.ends_with(".pdf") {
            Document::load_mem(&bytes).unwrap_or_else(|e| panic!("{name} unparseable: {e}"));
        }
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
    catalog_extra: Option<lopdf::Dictionary>,
}

impl Builder {
    fn new() -> Self {
        Builder {
            doc: Document::new(),
            pages_id: None,
            page_ids: Vec::new(),
            catalog_extra: None,
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
        let mut cat = dictionary! {
            "Type" => "Catalog",
            "Pages" => Object::Reference(pages),
        };
        if let Some(x) = self.catalog_extra.take() {
            for (k, v) in x.iter() {
                cat.set(k.clone(), v.clone());
            }
        }
        let catalog = self.add(cat);
        self.doc.trailer.set("Root", Object::Reference(catalog));
        // Classic xref table for maximum parser compatibility.
        self.doc.reference_table.cross_reference_type = XrefType::CrossReferenceTable;
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

    // ---- M3 incremental-save matrix -------------------------------------

    let mut b = Builder::new();
    two_page_text(&mut b);
    b.finish().save(dir.join("incr-classic.pdf")).unwrap();

    let mut b = Builder::new();
    two_page_text(&mut b);
    save_opts(
        b.finish(),
        dir.join("incr-xrefstream.pdf"),
        SaveOptions::builder()
            .use_object_streams(true)
            .use_xref_streams(true)
            .build(),
    );

    // lopdf accepts `linearize` but does not implement it, so a real
    // linearized file is produced through qpdf when available.
    {
        let mut b = Builder::new();
        two_page_text(&mut b);
        let tmp = dir.join("incr-linearized-src.pdf");
        b.finish().save(&tmp).unwrap();
        let out = dir.join("incr-linearized.pdf");
        let ok = std::process::Command::new("qpdf")
            .args(["--linearize"])
            .arg(&tmp)
            .arg(&out)
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        if ok {
            std::fs::remove_file(&tmp).unwrap();
        } else {
            eprintln!("qpdf unavailable; incr-linearized.pdf will not be linearized");
            std::fs::rename(&tmp, &out).unwrap();
        }
    }

    let mut b = Builder::new();
    presigned_pdf(&mut b);
    b.finish().save(dir.join("incr-presigned.pdf")).unwrap();

    let mut b = Builder::new();
    two_page_text(&mut b);
    let mut doc = b.finish();
    let mut base = Vec::new();
    doc.save_with_options(
        &mut base,
        SaveOptions::builder()
            .use_object_streams(true)
            .use_xref_streams(true)
            .build(),
    )
    .unwrap();
    std::fs::write(dir.join("incr-hybrid.pdf"), make_hybrid(&base, &doc)).unwrap();

    let mut b = Builder::new();
    annots_existing_pdf(&mut b);
    b.finish().save(dir.join("annots-existing.pdf")).unwrap();

    let mut png = image::RgbaImage::new(16, 16);
    for (x, y, p) in png.enumerate_pixels_mut() {
        *p = image::Rgba([(x * 16) as u8, (y * 16) as u8, 200, (x * 16) as u8]);
    }
    image::DynamicImage::ImageRgba8(png)
        .save(dir.join("stamp-src.png"))
        .unwrap();
    let mut jpg = image::RgbImage::new(16, 16);
    for (x, y, p) in jpg.enumerate_pixels_mut() {
        *p = image::Rgb([(x * 16) as u8, 128, (y * 16) as u8]);
    }
    image::DynamicImage::ImageRgb8(jpg)
        .save(dir.join("stamp-src.jpg"))
        .unwrap();
}

fn save_opts(mut doc: Document, path: PathBuf, opts: SaveOptions) {
    let mut f = std::fs::File::create(path).unwrap();
    doc.save_with_options(&mut f, opts).unwrap();
}

fn two_page_text(b: &mut Builder) {
    let font = b.font_helvetica();
    let resources = b.add(dictionary! {
        "Font" => dictionary! { "F1" => Object::Reference(font) },
    });
    for n in 1..=2 {
        let mut content = text_line(40.0, 160.0, 12.0, &format!("incr fixture page {n}"));
        content.extend(text_line(40.0, 140.0, 10.0, "body text"));
        let contents = b.stream(content);
        b.add_page([0.0, 0.0, 200.0, 200.0], None, None, contents, resources);
    }
}

fn presigned_pdf(b: &mut Builder) {
    two_page_text(b);
    let sig_dict = dictionary! {
        "Type" => "Sig",
        "Filter" => "Adobe.PPKLite",
        "SubFilter" => "adbe.pkcs7.detached",
        "ByteRange" => vec![
            Object::Integer(0),
            Object::Integer(1000),
            Object::Integer(2000),
            Object::Integer(3000),
        ],
        "Contents" => Object::String(vec![0u8; 64], StringFormat::Hexadecimal),
        "Reason" => Object::string_literal("fixture signature"),
    };
    let sig = b.add(sig_dict);
    let field = dictionary! {
        "FT" => "Sig",
        "T" => Object::string_literal("Signature1"),
        "Type" => "Annot",
        "Subtype" => "Widget",
        "Rect" => box4([10.0, 10.0, 60.0, 30.0]),
        "F" => 132,
        "P" => Object::Reference(b.page_ids[0]),
        "V" => Object::Reference(sig),
    };
    let field_id = b.add(field);
    let acro = b.add(dictionary! {
        "Fields" => vec![Object::Reference(field_id)],
        "SigFlags" => 3,
    });
    b.catalog_extra = Some(dictionary! {
        "AcroForm" => Object::Reference(acro),
    });
}

/// Appends a classic xref section + trailer with /XRefStm to a xref-stream
/// file, producing a hybrid-reference document.
fn make_hybrid(base: &[u8], doc: &Document) -> Vec<u8> {
    let tail = &base[base.len().saturating_sub(256)..];
    let tail_s = String::from_utf8_lossy(tail);
    let pos = tail_s.rfind("startxref").expect("startxref");
    let xref_off: usize = tail_s[pos + 9..]
        .split_whitespace()
        .next()
        .unwrap()
        .parse()
        .unwrap();
    let size = doc.max_id + 1;
    let root = match doc.trailer.get(b"Root").unwrap() {
        Object::Reference(r) => format!("{} {} R", r.0, r.1),
        _ => panic!("root not a reference"),
    };
    let mut out = base.to_vec();
    if !out.ends_with(b"\n") {
        out.push(b'\n');
    }
    let classic_off = out.len();
    out.extend_from_slice(
        format!(
            "xref\n0 1\n0000000000 65535 f \ntrailer\n<< /Size {size} /Root {root} /Prev {xref_off} /XRefStm {xref_off} >>\nstartxref\n{classic_off}\n%%EOF\n"
        )
        .as_bytes(),
    );
    out
}

/// One annot of each of the 11 subtypes on a 400x400 page, each with a
/// simple /AP /N form XObject. Two carry fixed /NM UUIDs; the Text annot
/// has a /Popup; the Highlight carries /NibData.
fn annots_existing_pdf(b: &mut Builder) {
    let font = b.font_helvetica();
    let resources = b.add(dictionary! {
        "Font" => dictionary! {
            "F1" => Object::Reference(font),
            "Helv" => dictionary! {
                "Type" => "Font",
                "Subtype" => "Type1",
                "BaseFont" => "Helvetica",
                "Encoding" => "WinAnsiEncoding",
            },
        },
    });
    let contents = b.stream(text_line(30.0, 200.0, 14.0, "annots-existing fixture"));
    b.add_page([0.0, 0.0, 400.0, 400.0], None, None, contents, resources);
    let page = b.page_ids[0];

    fn ap(b: &mut Builder, rect: [f64; 4]) -> ObjectId {
        let content = format!(
            "0.2 0.6 0.9 rg {} {} {} {} re f\n",
            rect[0],
            rect[1],
            rect[2] - rect[0],
            rect[3] - rect[1]
        );
        b.add(Stream::new(
            dictionary! {
                "Type" => "XObject",
                "Subtype" => "Form",
                "FormType" => 1,
                "BBox" => box4(rect),
                "Resources" => dictionary! {},
            },
            content.into_bytes(),
        ))
    }
    fn annot(b: &mut Builder, subtype: &str, rect: [f64; 4], extra: lopdf::Dictionary) -> ObjectId {
        let ap_id = ap(b, rect);
        let mut d = dictionary! {
            "Type" => "Annot",
            "Subtype" => subtype,
            "Rect" => box4(rect),
            "F" => 4,
            "AP" => dictionary! { "N" => Object::Reference(ap_id) },
        };
        for (k, v) in extra.iter() {
            d.set(k.clone(), v.clone());
        }
        b.add(Object::Dictionary(d))
    }

    let mut annots: Vec<ObjectId> = Vec::new();
    annots.push(annot(
        b,
        "Ink",
        [50.0, 50.0, 120.0, 90.0],
        dictionary! {
            "InkList" => vec![Object::Array(vec![
                Object::Real(55.0), Object::Real(60.0),
                Object::Real(80.0), Object::Real(75.0),
                Object::Real(110.0), Object::Real(65.0),
            ])],
            "BS" => dictionary! { "W" => 2.0, "S" => "S" },
        },
    ));
    annots.push(annot(
        b,
        "Highlight",
        [50.0, 100.0, 200.0, 112.0],
        dictionary! {
            "QuadPoints" => vec![
                Object::Real(50.0), Object::Real(112.0),
                Object::Real(200.0), Object::Real(112.0),
                Object::Real(50.0), Object::Real(100.0),
                Object::Real(200.0), Object::Real(100.0),
            ],
            "C" => vec![Object::Real(1.0), Object::Real(0.92), Object::Real(0.23)],
            "NM" => Object::string_literal("11111111-2222-4333-8444-555555555555"),
            "NibData" => Object::string_literal("{\"custom\":{\"k\":1},\"selectedText\":\"abc\"}"),
        },
    ));
    annots.push(annot(
        b,
        "Underline",
        [50.0, 120.0, 200.0, 132.0],
        dictionary! {
            "QuadPoints" => vec![
                Object::Real(50.0), Object::Real(132.0),
                Object::Real(200.0), Object::Real(132.0),
                Object::Real(50.0), Object::Real(120.0),
                Object::Real(200.0), Object::Real(120.0),
            ],
            "C" => vec![Object::Real(0.1), Object::Real(0.1), Object::Real(0.9)],
        },
    ));
    annots.push(annot(
        b,
        "StrikeOut",
        [50.0, 140.0, 200.0, 152.0],
        dictionary! {
            "QuadPoints" => vec![
                Object::Real(50.0), Object::Real(152.0),
                Object::Real(200.0), Object::Real(152.0),
                Object::Real(50.0), Object::Real(140.0),
                Object::Real(200.0), Object::Real(140.0),
            ],
        },
    ));
    annots.push(annot(
        b,
        "Squiggly",
        [50.0, 160.0, 200.0, 172.0],
        dictionary! {
            "QuadPoints" => vec![
                Object::Real(50.0), Object::Real(172.0),
                Object::Real(200.0), Object::Real(172.0),
                Object::Real(50.0), Object::Real(160.0),
                Object::Real(200.0), Object::Real(160.0),
            ],
        },
    ));
    annots.push(annot(
        b,
        "FreeText",
        [220.0, 50.0, 380.0, 90.0],
        dictionary! {
            "DA" => Object::string_literal("/Helv 12 Tf 0 0 0 rg"),
            "Q" => 0,
            "Contents" => Object::string_literal("hello freetext"),
            "C" => vec![Object::Real(0.8), Object::Real(0.1), Object::Real(0.1)],
            "IC" => vec![Object::Real(1.0), Object::Real(1.0), Object::Real(1.0)],
            "BS" => dictionary! { "W" => 1.0, "S" => "D", "D" => vec![Object::Integer(3), Object::Integer(2)] },
        },
    ));
    let popup = b.add(Object::Dictionary(dictionary! {
        "Type" => "Annot",
        "Subtype" => "Popup",
        "Rect" => box4([310.0, 100.0, 390.0, 150.0]),
        "Open" => false,
    }));
    annots.push(annot(
        b,
        "Text",
        [300.0, 100.0, 320.0, 120.0],
        dictionary! {
            "Name" => "Comment",
            "Open" => false,
            "Contents" => Object::string_literal("sticky note"),
            "Popup" => Object::Reference(popup),
        },
    ));
    annots.push(annot(
        b,
        "Stamp",
        [50.0, 200.0, 170.0, 250.0],
        dictionary! { "Name" => "Approved" },
    ));
    annots.push(annot(
        b,
        "Square",
        [200.0, 200.0, 280.0, 260.0],
        dictionary! {
            "C" => vec![Object::Real(0.83), Object::Real(0.18), Object::Real(0.18)],
            "IC" => vec![Object::Real(0.9), Object::Real(0.9), Object::Real(0.6)],
            "BS" => dictionary! { "W" => 2.0, "S" => "S" },
        },
    ));
    annots.push(annot(
        b,
        "Circle",
        [300.0, 200.0, 380.0, 280.0],
        dictionary! {
            "C" => vec![Object::Real(0.83), Object::Real(0.18), Object::Real(0.18)],
            "BS" => dictionary! { "W" => 2.0, "S" => "D", "D" => vec![Object::Integer(8), Object::Integer(6)] },
        },
    ));
    annots.push(annot(
        b,
        "Line",
        [50.0, 300.0, 300.0, 350.0],
        dictionary! {
            "L" => vec![
                Object::Real(50.0), Object::Real(300.0),
                Object::Real(300.0), Object::Real(350.0),
            ],
            "LE" => vec![Object::Name(b"None".to_vec()), Object::Name(b"ClosedArrow".to_vec())],
            "BS" => dictionary! { "W" => 1.5, "S" => "S" },
            "NM" => Object::string_literal("22222222-3333-4444-8555-666666666666"),
        },
    ));

    let page_dict = b.doc.get_object_mut(page).unwrap().as_dict_mut().unwrap();
    page_dict.set(
        "Annots",
        Object::Array(annots.iter().map(|i| Object::Reference(*i)).collect()),
    );
}
