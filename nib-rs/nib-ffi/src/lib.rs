#![doc = "nib-ffi: UniFFI surface for nib — thin facade over nib-core, nib-render, nib-sign."]
#![forbid(unsafe_code)]

mod document;
mod error;

pub use document::{
    AnnotationSummary, ApplyReport, AssetProvider, DeltaValidationError, DirtyRegion, ExportMode,
    FlushMode, FlushOptions, FlushReport, FontProvider, FormFieldSummary, FormFieldValue, IRect,
    ISize, ImportOptions, NibDocument, OpenOptions, PageInfo, Point, Priority, Rect, Rotation,
    SignatureOptions, SignatureRequest, SigningAlgorithm, SigningDigest, TileKey, TileRequest,
    TileResult,
};
pub use error::NibError;

uniffi::setup_scaffolding!();

#[cfg(target_os = "android")]
fn init_logger() {
    use std::sync::Once;
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        android_logger::init_once(android_logger::Config::default().with_tag("nib"));
    });
}

#[cfg(not(target_os = "android"))]
fn init_logger() {}

#[uniffi::export]
pub fn nib_version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

#[uniffi::export]
pub fn pdfium_version() -> Result<String, NibError> {
    init_logger();
    match std::panic::catch_unwind(|| nib_render::pdfium_probe(None)) {
        Ok(Ok(s)) => Ok(s),
        Ok(Err(nib_render::ProbeError::Pdfium(m))) => Err(NibError::Pdfium(m)),
        Err(_) => Err(NibError::Internal("panic in pdfium_probe".into())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsString;
    use std::path::{Path, PathBuf};
    use std::process::Command;
    use std::sync::Arc;

    #[test]
    fn version_matches_cargo() {
        assert_eq!(super::nib_version(), env!("CARGO_PKG_VERSION"));
    }

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
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../target/pdfium")
            .join(triple)
    }

    /// `worker()` binds the already-mapped library unless NIB_PDFIUM_DIR is
    /// set — set it before the first document is opened.
    fn open_text() -> Arc<NibDocument> {
        std::env::set_var("NIB_PDFIUM_DIR", host_pdfium_dir());
        let bytes = std::fs::read(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../nib-core/tests/fixtures/text.pdf"),
        )
        .unwrap();
        NibDocument::from_bytes(
            bytes,
            OpenOptions {
                lazy_structure: false,
            },
        )
        .unwrap()
    }

    fn open_signature_fixture() -> Arc<NibDocument> {
        std::env::set_var("NIB_PDFIUM_DIR", host_pdfium_dir());
        let bytes = std::fs::read(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../nib-core/tests/fixtures/sig-field.pdf"),
        )
        .unwrap();
        NibDocument::from_bytes(
            bytes,
            OpenOptions {
                lazy_structure: false,
            },
        )
        .unwrap()
    }

    fn create_ecdsa_signer() -> (tempfile::TempDir, Vec<u8>, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let key = dir.path().join("key.pem");
        let cert_pem = dir.path().join("cert.pem");
        let cert_der = dir.path().join("cert.der");
        let openssl = std::env::var_os("NIB_OPENSSL").unwrap_or_else(|| OsString::from("openssl"));
        run_openssl(
            &openssl,
            [
                OsString::from("ecparam"),
                OsString::from("-name"),
                OsString::from("prime256v1"),
                OsString::from("-genkey"),
                OsString::from("-noout"),
                OsString::from("-out"),
                key.as_os_str().to_owned(),
            ],
        );
        run_openssl(
            &openssl,
            [
                OsString::from("req"),
                OsString::from("-new"),
                OsString::from("-x509"),
                OsString::from("-key"),
                key.as_os_str().to_owned(),
                OsString::from("-out"),
                cert_pem.as_os_str().to_owned(),
                OsString::from("-days"),
                OsString::from("1"),
                OsString::from("-subj"),
                OsString::from("/CN=Nib FFI test signer"),
            ],
        );
        run_openssl(
            &openssl,
            [
                OsString::from("x509"),
                OsString::from("-in"),
                cert_pem.as_os_str().to_owned(),
                OsString::from("-outform"),
                OsString::from("DER"),
                OsString::from("-out"),
                cert_der.as_os_str().to_owned(),
            ],
        );
        (dir, std::fs::read(cert_der).unwrap(), key)
    }

    fn run_openssl(executable: &std::ffi::OsStr, args: impl IntoIterator<Item = OsString>) {
        let output = Command::new(executable).args(args).output().unwrap();
        assert!(
            output.status.success(),
            "openssl failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn openssl_sign(key: &Path, data: &[u8], dir: &Path) -> Vec<u8> {
        let data_file = dir.join("signed-attributes.der");
        let signature_file = dir.join("signature.der");
        std::fs::write(&data_file, data).unwrap();
        let openssl = std::env::var_os("NIB_OPENSSL").unwrap_or_else(|| OsString::from("openssl"));
        run_openssl(
            &openssl,
            [
                OsString::from("dgst"),
                OsString::from("-sha256"),
                OsString::from("-sign"),
                key.as_os_str().to_owned(),
                OsString::from("-out"),
                signature_file.as_os_str().to_owned(),
                data_file.as_os_str().to_owned(),
            ],
        );
        std::fs::read(signature_file).unwrap()
    }

    fn ffi_signature_options(certificate: Vec<u8>) -> SignatureOptions {
        SignatureOptions {
            certificate,
            field_name: Some("Signature1".into()),
            page: 0,
            algorithm: SigningAlgorithm::EcdsaP256,
            digest: SigningDigest::Sha256,
            reason: Some("FFI signing test".into()),
            location: None,
            contact_info: None,
            contents_hex_len: 0,
            appearance_mode: None,
            appearance_description: None,
            appearance_graphic_ref: None,
            appearance_font_size: None,
        }
    }

    const CREATE_SQUARE: &str = r##"{
        "opId": "0193b2e4-7c1a-7f2e-9a53-6d2f8a1b0c01",
        "ts": "2026-11-12T14:03:25Z",
        "type": "create",
        "target": "annotation",
        "id": "c0a8012e-5b7d-4d3e-8c4f-2f6b9d1e7a55",
        "page": 0,
        "payload": {"subtype": "Square", "page": 0, "rect": [10, 10, 50, 50], "color": "#FF0000"}
    }"##;

    #[test]
    fn apply_undo_redo_roundtrip() {
        let doc = open_text();
        let rep = doc.apply(format!("[{CREATE_SQUARE}]")).unwrap();
        assert_eq!(rep.applied.len(), 1);
        assert_eq!(doc.annotations(0).len(), 1);
        assert_eq!(doc.annotations(0)[0].subtype, "Square");
        assert!(doc.can_undo());
        assert!(doc.undo().is_some());
        assert_eq!(doc.annotations(0).len(), 0);
        assert!(doc.can_redo());
        assert!(doc.redo().is_some());
        assert_eq!(doc.annotations(0).len(), 1);
    }

    #[test]
    fn signature_prepare_failure_and_unknown_abort_publish_nothing() {
        let doc = open_text();
        let original = doc.bytes().unwrap();
        let result = doc.prepare_signature(SignatureOptions {
            certificate: Vec::new(),
            field_name: None,
            page: 0,
            algorithm: SigningAlgorithm::EcdsaP256,
            digest: SigningDigest::Sha256,
            reason: None,
            location: None,
            contact_info: None,
            contents_hex_len: 0,
            appearance_mode: None,
            appearance_description: None,
            appearance_graphic_ref: None,
            appearance_font_size: None,
        });
        assert!(matches!(result, Err(NibError::Signature(_))));
        assert_eq!(doc.revision(), 0);
        assert_eq!(doc.bytes().unwrap(), original);

        assert!(matches!(
            doc.abort_signature("missing-token".into()),
            Err(NibError::Signature(_))
        ));
        assert_eq!(doc.bytes().unwrap(), original);
    }

    #[test]
    fn transactional_signature_is_private_until_verified_completion() {
        let (temp, certificate, key) = create_ecdsa_signer();
        let doc = open_signature_fixture();
        let original = doc.bytes().unwrap();
        let prepared = doc
            .prepare_signature(ffi_signature_options(certificate.clone()))
            .unwrap();
        assert_eq!(doc.bytes().unwrap(), original);
        assert_eq!(doc.revision(), 0);

        let bad = doc
            .complete_signature(prepared.token.clone(), vec![1], vec![certificate.clone()])
            .unwrap_err();
        assert!(matches!(bad, NibError::Signature(_)));
        assert_eq!(doc.bytes().unwrap(), original);

        let signature = openssl_sign(&key, &prepared.data, temp.path());
        let report = doc
            .complete_signature(prepared.token, signature, vec![certificate])
            .unwrap();
        assert_eq!(report.revision, 1);
        let signed = doc.bytes().unwrap();
        assert_ne!(signed, original);
        let verifications = nib_sign::verify_signatures(&signed, &[]).unwrap();
        assert_eq!(verifications.len(), 1);
        assert_eq!(verifications[0].integrity, nib_sign::IntegrityStatus::Valid);
        let field: nib_core::model::FormField =
            serde_json::from_str(&doc.form_field_json("Signature1".into()).unwrap()).unwrap();
        assert_eq!(
            field.signature_state,
            Some(nib_core::model::SignatureFieldState::Occupied)
        );
    }

    #[test]
    fn engine_edit_while_signer_runs_makes_preparation_stale() {
        let (_temp, certificate, key) = create_ecdsa_signer();
        let doc = open_signature_fixture();
        let original = doc.bytes().unwrap();
        let prepared = doc
            .prepare_signature(ffi_signature_options(certificate.clone()))
            .unwrap();

        let signature = openssl_sign(&key, &prepared.data, _temp.path());
        let edit = nib_core::model::Op {
            op_id: uuid::Uuid::now_v7(),
            ts: chrono::Utc::now(),
            author: None,
            group_id: None,
            kind: nib_core::model::OpKind::Create,
            target: nib_core::model::Target::Annotation,
            id: "c0a8012e-5b7d-4d3e-8c4f-2f6b9d1e7a55".into(),
            page: Some(0),
            payload: serde_json::json!({
                "subtype": "Square",
                "page": 0,
                "rect": [10, 10, 50, 50],
                "color": "#FF0000"
            }),
        };
        assert_eq!(
            doc.apply(serde_json::to_string(&edit).unwrap())
                .unwrap()
                .applied
                .len(),
            1
        );

        let error = doc
            .complete_signature(prepared.token, signature, vec![certificate])
            .unwrap_err();
        assert!(matches!(error, NibError::StaleSignature(_)));
        assert_eq!(doc.bytes().unwrap(), original);
        assert_eq!(doc.annotations(0).len(), 1);
    }

    #[test]
    fn text_lines_in_rect_two_lines() {
        let doc = open_text();
        let lines = doc
            .text_lines_in_rect(
                0,
                Rect {
                    x0: 30.0,
                    y0: 140.0,
                    x1: 190.0,
                    y1: 165.0,
                },
            )
            .unwrap();
        assert_eq!(lines.len(), 2, "lines: {lines:?}");
        assert!((lines[0].y1 - lines[1].y1 - 16.0).abs() <= 0.75);
    }

    #[test]
    fn invalid_delta_error_is_json() {
        let doc = open_text();
        let bad = CREATE_SQUARE.replace("\"page\": 0", "\"page\": 99");
        match doc.apply(format!("[{bad}]")) {
            Err(NibError::InvalidDelta(s)) => {
                let v: serde_json::Value = serde_json::from_str(&s).unwrap();
                let arr = v.as_array().unwrap();
                assert_eq!(arr[0]["code"], "PAGE_OUT_OF_RANGE");
            }
            other => panic!("expected InvalidDelta, got {other:?}"),
        }
    }
}

#[cfg(test)]
mod flush_tests {
    use super::*;
    use std::path::{Path, PathBuf};
    use std::pin::pin;
    use std::sync::Arc;
    use std::task::{Context, Poll, Waker};

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
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../target/pdfium")
            .join(triple)
    }

    fn fixture(name: &str) -> Vec<u8> {
        std::fs::read(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../nib-core/tests/fixtures")
                .join(name),
        )
        .unwrap()
    }

    fn open(name: &str, lazy: bool) -> Arc<NibDocument> {
        std::env::set_var("NIB_PDFIUM_DIR", host_pdfium_dir());
        NibDocument::from_bytes(
            fixture(name),
            OpenOptions {
                lazy_structure: lazy,
            },
        )
        .unwrap()
    }

    fn block_on<F: std::future::Future>(f: F) -> F::Output {
        let waker = Waker::noop();
        let mut cx = Context::from_waker(waker);
        let mut f = pin!(f);
        loop {
            if let Poll::Ready(v) = f.as_mut().poll(&mut cx) {
                return v;
            }
            std::thread::yield_now();
        }
    }

    fn render_first_page(doc: &NibDocument, w: u32, h: u32) -> TileResult {
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
                width: w as i32,
                height: h as i32,
            },
            page_device_size: ISize {
                width: w as i32,
                height: h as i32,
            },
            generation: u64::MAX,
            priority: Priority::Visible,
            distance: 0,
        };
        block_on(doc.render_tile(req)).unwrap()
    }

    static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

    fn op(id: &str, kind: &str, payload: &str) -> String {
        let n = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        format!(
            r#"{{"opId":"0193b2e4-7c1a-7f2e-9a53-6d{n:010x}","ts":"2026-11-12T14:03:25Z","type":"{kind}","target":"annotation","id":"{id}","page":0,"payload":{payload}}}"#
        )
    }

    const SQ_ID: &str = "c0a8012e-5b7d-4d3e-8c4f-2f6b9d1e7a55";
    const CREATE_SQUARE: &str =
        r##"{"subtype": "Square", "page": 0, "rect": [10, 10, 50, 50], "color": "#FF0000"}"##;

    #[test]
    fn flush_then_reopen_counts() {
        let doc = open("text.pdf", false);
        doc.apply(format!("[{}]", op(SQ_ID, "create", CREATE_SQUARE)))
            .unwrap();
        let rep = doc.flush(FlushMode::Incremental).unwrap();
        assert_eq!(rep.revision, 1);
        assert_eq!(rep.annots_written, 1);
        let reopened = NibDocument::from_bytes(
            doc.bytes().unwrap(),
            OpenOptions {
                lazy_structure: false,
            },
        )
        .unwrap();
        assert_eq!(reopened.annotations(0).len(), 1);
        assert_eq!(reopened.annotations(0)[0].id, SQ_ID);
        let delta: serde_json::Value =
            serde_json::from_str(&reopened.export_delta(ExportMode::Compacted)).unwrap();
        assert_eq!(delta["ops"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn flush_noop_when_clean() {
        let doc = open("text.pdf", false);
        doc.apply(format!("[{}]", op(SQ_ID, "create", CREATE_SQUARE)))
            .unwrap();
        let r1 = doc.flush(FlushMode::Incremental).unwrap();
        let r2 = doc.flush(FlushMode::Incremental).unwrap();
        assert_eq!(r2.revision, r1.revision);
        assert_eq!(r2.bytes_written, 0);
        assert!(doc.uncommitted_ids().is_empty());
    }

    #[test]
    fn incremental_matrix_pdfium_reload() {
        for name in [
            "incr-classic.pdf",
            "incr-xrefstream.pdf",
            "incr-linearized.pdf",
            "incr-presigned.pdf",
            "incr-hybrid.pdf",
        ] {
            let doc = open(name, false);
            // create
            doc.apply(format!("[{}]", op(SQ_ID, "create", CREATE_SQUARE)))
                .unwrap_or_else(|e| panic!("{name}: apply create: {e:?}"));
            let r = doc.flush(FlushMode::Incremental).unwrap();
            let re = NibDocument::from_bytes(
                doc.bytes().unwrap(),
                OpenOptions {
                    lazy_structure: false,
                },
            )
            .unwrap_or_else(|e| panic!("{name}: reopen after create: {e:?}"));
            assert_eq!(re.annotations(0).len(), 1, "{name}");
            let t = render_first_page(&re, 100, 100);
            assert_eq!(t.pixels.len(), 100 * 100 * 4);
            // update colour
            doc.apply(format!(
                "[{}]",
                op(SQ_ID, "update", r##"{"color":"#00FF00"}"##)
            ))
            .unwrap();
            doc.flush(FlushMode::Incremental).unwrap();
            let re = NibDocument::from_bytes(
                doc.bytes().unwrap(),
                OpenOptions {
                    lazy_structure: false,
                },
            )
            .unwrap_or_else(|e| panic!("{name}: reopen after update: {e:?}"));
            let t = render_first_page(&re, 100, 100);
            assert_eq!(t.pixels.len(), 100 * 100 * 4);
            // delete
            doc.apply(format!("[{}]", op(SQ_ID, "delete", "{}")))
                .unwrap();
            doc.flush(FlushMode::Incremental).unwrap();
            let re = NibDocument::from_bytes(
                doc.bytes().unwrap(),
                OpenOptions {
                    lazy_structure: false,
                },
            )
            .unwrap_or_else(|e| panic!("{name}: reopen after delete: {e:?}"));
            assert_eq!(re.annotations(0).len(), 0, "{name}");
            let t = render_first_page(&re, 100, 100);
            assert_eq!(t.pixels.len(), 100 * 100 * 4);
            let _ = r;
        }
    }

    #[test]
    fn lazy_structure_defers_import() {
        let doc = open("annots-existing.pdf", true);
        assert_eq!(doc.annotations(0).len(), 11);
        // And a subsequent flush reports zero ops to write.
        let rep = doc.flush(FlushMode::Incremental).unwrap();
        assert_eq!(rep.bytes_written, 0);
    }

    #[test]
    fn visual_signature_imports_lazy_fields_and_replays() {
        let payload = r##"{"subtype":"Ink","page":0,"intent":"signature","bindToField":"Signature1","fitMode":"contain","strokeWidth":2,"strokes":[{"points":[[0,0],[100,20]]}]}"##;
        let doc = open("sig-field.pdf", true);
        doc.apply(op(SQ_ID, "create", payload)).unwrap();
        let annotation: serde_json::Value =
            serde_json::from_str(&doc.annotation_json(SQ_ID.into()).unwrap()).unwrap();
        assert_eq!(
            annotation["rect"],
            serde_json::json!([40.0, 70.0, 260.0, 150.0])
        );
        assert_eq!(annotation["flags"]["locked"], true);
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(
                &doc.form_field_json("Signature1".into()).unwrap()
            )
            .unwrap()["signatureState"],
            "empty"
        );
        assert!(doc.undo().is_some());
        assert!(doc.annotation_json(SQ_ID.into()).is_none());
        assert!(doc.redo().is_some());
        for mode in [ExportMode::Full, ExportMode::Compacted] {
            let replay = open("sig-field.pdf", true);
            replay
                .import_delta(
                    doc.export_delta(mode),
                    ImportOptions {
                        force: false,
                        rebase: false,
                        undoable: true,
                    },
                )
                .unwrap();
            let actual: serde_json::Value =
                serde_json::from_str(&replay.annotation_json(SQ_ID.into()).unwrap()).unwrap();
            assert_eq!(actual["rect"], annotation["rect"]);
            for (actual_point, expected_point) in actual["strokes"][0]["points"]
                .as_array()
                .unwrap()
                .iter()
                .zip(annotation["strokes"][0]["points"].as_array().unwrap())
            {
                for index in 0..2 {
                    let difference = actual_point[index].as_f64().unwrap()
                        - expected_point[index].as_f64().unwrap();
                    assert!(
                        difference.abs() <= 0.001,
                        "coordinate difference: {difference}"
                    );
                }
            }
        }
        doc.flush(FlushMode::Incremental).unwrap();
        let reopened = NibDocument::from_bytes(
            doc.bytes().unwrap(),
            OpenOptions {
                lazy_structure: true,
            },
        )
        .unwrap();
        let restored: serde_json::Value =
            serde_json::from_str(&reopened.annotation_json(SQ_ID.into()).unwrap()).unwrap();
        assert_eq!(restored["rect"], annotation["rect"]);
        assert_eq!(restored["bindToField"], "Signature1");
        assert_eq!(restored["fitMode"], "contain");
        assert_eq!(restored["flags"]["locked"], true);

        let bad = payload.replace("Signature1", "Missing");
        let error = doc.apply(op("c0a8012e-5b7d-4d3e-8c4f-2f6b9d1e7a56", "create", &bad));
        assert!(matches!(error, Err(NibError::InvalidDelta(_))));
    }

    struct Png;
    impl AssetProvider for Png {
        fn asset(&self, asset_id: String) -> Option<Vec<u8>> {
            (asset_id == "a1").then(|| fixture("stamp-src.png"))
        }
    }

    #[test]
    fn asset_provider_used_for_stamp() {
        let doc = open("text.pdf", false);
        doc.set_asset_provider(Some(Arc::new(Png)));
        let payload = r#"{"subtype":"Stamp","page":0,"rect":[50,50,150,120],"kind":"image","image":{"assetId":"a1"}}"#;
        doc.apply(format!("[{}]", op(SQ_ID, "create", payload)))
            .unwrap();
        let rep = doc.flush(FlushMode::Incremental).unwrap();
        assert!(rep.warnings.is_empty(), "warnings: {:?}", rep.warnings);

        let pdf = lopdf::Document::load_mem(&doc.bytes().unwrap()).unwrap();
        let page = pdf.get_pages()[&1];
        let annots = pdf
            .get_object(page)
            .unwrap()
            .as_dict()
            .unwrap()
            .get(b"Annots")
            .unwrap()
            .as_array()
            .unwrap();
        let a = pdf
            .get_object(annots[0].as_reference().unwrap())
            .unwrap()
            .as_dict()
            .unwrap();
        let ap = a.get(b"AP").unwrap().as_dict().unwrap();
        let n = pdf
            .get_object(ap.get(b"N").unwrap().as_reference().unwrap())
            .unwrap()
            .as_stream()
            .unwrap();
        let res = n.dict.get(b"Resources").unwrap().as_dict().unwrap();
        let xo = res.get(b"XObject").unwrap().as_dict().unwrap();
        assert!(!xo.is_empty(), "no image XObject in /AP resources");
    }

    #[test]
    fn signature_ops_are_rejected_at_apply_and_import() {
        let doc = open("sig-field.pdf", true);
        let sig_op = op(SQ_ID, "create", r#"{"field":"Signature1"}"#)
            .replace(r#""target":"annotation""#, r#""target":"signature""#);
        let batch = format!("[{}, {sig_op}]", op(SQ_ID, "create", CREATE_SQUARE));
        let Err(NibError::InvalidDelta(json)) = doc.apply(batch) else {
            panic!("signature op accepted by apply");
        };
        let errs: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(errs[0]["code"], "UNSUPPORTED_OP");
        assert_eq!(errs[0]["opIndex"], 1);
        // Atomic: the valid square in the same batch was not applied.
        assert!(doc.annotation_json(SQ_ID.into()).is_none());
        assert!(!doc.can_undo());

        let mut delta: serde_json::Value =
            serde_json::from_str(&doc.export_delta(ExportMode::Full)).unwrap();
        delta["ops"]
            .as_array_mut()
            .unwrap()
            .push(serde_json::from_str(&sig_op).unwrap());
        let Err(NibError::InvalidDelta(json)) = doc.import_delta(
            delta.to_string(),
            ImportOptions {
                force: false,
                rebase: false,
                undoable: true,
            },
        ) else {
            panic!("signature op accepted by import_delta");
        };
        assert!(json.contains("UNSUPPORTED_OP"));
        let rep = doc.flush(FlushMode::Incremental).unwrap();
        assert_eq!(rep.annots_written, 0);
    }
}

#[cfg(test)]
mod form_tests {
    use super::*;
    use std::collections::BTreeMap;
    use std::path::{Path, PathBuf};
    use std::sync::Arc;

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
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../target/pdfium")
            .join(triple)
    }

    fn open_form() -> Arc<NibDocument> {
        std::env::set_var("NIB_PDFIUM_DIR", host_pdfium_dir());
        NibDocument::from_bytes(
            std::fs::read(
                Path::new(env!("CARGO_MANIFEST_DIR")).join("../nib-core/tests/fixtures/form.pdf"),
            )
            .unwrap(),
            OpenOptions {
                lazy_structure: false,
            },
        )
        .unwrap()
    }

    const VALUES: &[(&str, &str)] = &[
        ("name", "\"Andrea Rosa\""),
        ("notes", "\"first line\\nsecond line\""),
        ("code", "\"123456\""),
        ("secret", "\"hunter2\""),
        ("agree", "\"Yes\""),
        ("newsletter", "\"On\""),
        ("color", "\"Green\""),
        ("country", "\"IT\""),
        ("city", "\"Turin\""),
        ("plan", "[\"Pro\",\"Enterprise\"]"),
        ("bottom.note", "\"bottom text\""),
    ];

    fn set_all(doc: &NibDocument) {
        for (n, v) in VALUES {
            doc.set_field_value(n.to_string(), v.to_string())
                .unwrap_or_else(|e| panic!("set {n}: {e:?}"));
        }
    }

    fn values_map(doc: &NibDocument) -> BTreeMap<String, String> {
        doc.form_fields()
            .into_iter()
            .map(|f| (f.name, f.value_json))
            .collect()
    }

    #[test]
    fn form_fields_lists_fixture() {
        let doc = open_form();
        let fields = doc.form_fields();
        let names: Vec<&str> = fields.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(names.len(), 12, "{names:?}");
        assert!(names.contains(&"bottom.note"));
        let mut types = BTreeMap::<&str, usize>::new();
        for f in &fields {
            *types.entry(f.field_type.as_str()).or_default() += 1;
        }
        assert_eq!(types["text"], 6, "{types:?}");
        assert_eq!(types["checkbox"], 2);
        assert_eq!(types["radio"], 1);
        assert_eq!(types["combo"], 2);
        assert_eq!(types["list"], 1);
        let ro = fields.iter().find(|f| f.name == "readonly").unwrap();
        let j: serde_json::Value = serde_json::from_str(&ro.json).unwrap();
        assert_eq!(j["flags"]["readOnly"], true);
    }

    #[test]
    fn set_field_value_flush_reopen_values_equal() {
        let doc = open_form();
        set_all(&doc);
        let rep = doc.flush(FlushMode::Incremental).unwrap();
        assert!(rep.bytes_written > 0);
        assert_eq!(rep.fields_written, 11, "{rep:?}");
        let re = NibDocument::from_bytes(
            doc.bytes().unwrap(),
            OpenOptions {
                lazy_structure: false,
            },
        )
        .unwrap();
        let got = values_map(&re);
        for (n, v) in VALUES {
            assert_eq!(&got[*n], v, "{n}");
        }
        // pdfium read-back of the reopened bytes. Radio kids all report
        // the group name "color"; pdfium reports the option *label* for
        // combos ("Italy"), and an editable combo's free text reads None.
        let vals = re.form_field_values().unwrap();
        let by_name =
            |n: &str| -> Vec<&FormFieldValue> { vals.iter().filter(|v| v.name == n).collect() };
        assert_eq!(by_name("name")[0].value.as_deref(), Some("Andrea Rosa"));
        assert_eq!(by_name("code")[0].value.as_deref(), Some("123456"));
        assert_eq!(by_name("agree")[0].checked, Some(true));
        assert_eq!(by_name("newsletter")[0].checked, Some(true));
        let color = by_name("color");
        let checked: Vec<_> = color.iter().filter(|v| v.checked == Some(true)).collect();
        assert_eq!(checked.len(), 1, "color checked: {color:?}");
        assert_eq!(checked[0].value.as_deref(), Some("Green"));
        assert_eq!(by_name("country")[0].value.as_deref(), Some("Italy"));
    }

    #[test]
    fn form_validation_codes_through_ffi() {
        let doc = open_form();
        let code_of = |name: &str, v: &str| -> String {
            match doc.set_field_value(name.into(), v.into()) {
                Err(NibError::InvalidDelta(s)) => {
                    let j: serde_json::Value = serde_json::from_str(&s).unwrap();
                    j[0]["code"].as_str().unwrap().to_string()
                }
                other => panic!("{name}: expected InvalidDelta, got {other:?}"),
            }
        };
        assert_eq!(code_of("readonly", "\"x\""), "FIELD_READONLY");
        assert_eq!(code_of("country", "\"XX\""), "VALUE_NOT_IN_OPTIONS");
        assert_eq!(
            code_of("name", &format!("\"{}\"", "x".repeat(21))),
            "MAXLEN_EXCEEDED"
        );
    }

    #[test]
    fn flush_with_flatten_reports_and_empties_fields() {
        let doc = open_form();
        set_all(&doc);
        let rep = doc
            .flush_with(FlushOptions {
                mode: FlushMode::FullRewrite,
                flatten: true,
            })
            .unwrap();
        assert!(rep.flattened, "{rep:?}");
        assert!(doc.form_fields().is_empty());
        let re = NibDocument::from_bytes(
            doc.bytes().unwrap(),
            OpenOptions {
                lazy_structure: false,
            },
        )
        .unwrap();
        assert_eq!(re.form_fields().len(), 0);
        assert!(re.form_field_values().unwrap().is_empty());
    }
}
