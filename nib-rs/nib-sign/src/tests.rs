use std::path::Path;
use std::process::Command;
use std::sync::OnceLock;

use ::cms::content_info::ContentInfo;
use ::cms::signed_data::{SignedData, SignerInfos};
use const_oid::db::{rfc5911, rfc5912};
use der::asn1::{BitString, OctetString, SetOfVec, UtcTime};
use der::{Any, Decode, Encode, Sequence};
use getrandom::SysRng;
use lopdf::{Document, Object};
use nib_core::model::{FieldType, SignatureFieldState};
use p256::elliptic_curve::Generate;
use sha2::{Digest, Sha256, Sha384, Sha512};
use signature::hazmat::PrehashSigner;
use signature::Signer;
use spki::{AlgorithmIdentifierOwned, EncodePublicKey, SubjectPublicKeyInfoOwned};
use x509_cert::attr::Attribute;
use x509_cert::name::Name;
use x509_cert::serial_number::SerialNumber;
use x509_cert::time::Validity;

use super::*;

// ---------------------------------------------------------------- keys/certs

enum Key {
    Rsa(Box<rsa::PrivateKey>),
    P256(p256::ecdsa::SigningKey),
    P384(p384::ecdsa::SigningKey),
}

struct TestSigner {
    key: Key,
    cert: Vec<u8>,
}

/// v1 TBSCertificate (no version, no extensions) — `x509-cert` keeps its
/// own fields crate-private, so tests encode this layout directly.
#[derive(Sequence)]
struct Tbs {
    serial_number: SerialNumber,
    signature: AlgorithmIdentifierOwned,
    issuer: Name,
    validity: Validity,
    subject: Name,
    spki: SubjectPublicKeyInfoOwned,
}

#[derive(Sequence)]
struct RawCert {
    tbs: Tbs,
    alg: AlgorithmIdentifierOwned,
    sig: BitString,
}

fn alg(oid: const_oid::ObjectIdentifier, null: bool) -> AlgorithmIdentifierOwned {
    AlgorithmIdentifierOwned {
        oid,
        parameters: null.then(|| Any::from(der::asn1::Null)),
    }
}

fn hash(digest: DigestAlgorithm, data: &[u8]) -> Vec<u8> {
    match digest {
        DigestAlgorithm::Sha256 => Sha256::digest(data).to_vec(),
        DigestAlgorithm::Sha384 => Sha384::digest(data).to_vec(),
        DigestAlgorithm::Sha512 => Sha512::digest(data).to_vec(),
    }
}

impl TestSigner {
    fn spki(&self) -> SubjectPublicKeyInfoOwned {
        let doc = match &self.key {
            Key::Rsa(k) => k.to_public_key().to_public_key_der().unwrap(),
            Key::P256(k) => k.verifying_key().to_public_key_der().unwrap(),
            Key::P384(k) => k.verifying_key().to_public_key_der().unwrap(),
        };
        SubjectPublicKeyInfoOwned::from_der(doc.as_bytes()).unwrap()
    }

    /// Self-signed v1 certificate (RSA: sha256WithRSAEncryption, EC:
    /// ecdsa-with-SHA256/384).
    fn with_cert(key: Key, cn: &str, serial: u8) -> TestSigner {
        let mut s = TestSigner {
            key,
            cert: Vec::new(),
        };
        let sig_alg = match &s.key {
            Key::Rsa(_) => alg(rfc5912::SHA_256_WITH_RSA_ENCRYPTION, true),
            Key::P256(_) => alg(rfc5912::ECDSA_WITH_SHA_256, false),
            Key::P384(_) => alg(rfc5912::ECDSA_WITH_SHA_384, false),
        };
        let name: Name = format!("CN={cn}").parse().unwrap();
        let tbs = Tbs {
            serial_number: SerialNumber::new(&[serial]).unwrap(),
            signature: sig_alg.clone(),
            issuer: name.clone(),
            validity: Validity::from_now(std::time::Duration::from_secs(86_400)).unwrap(),
            subject: name,
            spki: s.spki(),
        };
        let tbs_der = tbs.to_der().unwrap();
        let sig = match &s.key {
            Key::Rsa(k) => k
                .sign(
                    rsa::Pkcs1v15Sign::new::<Sha256>(),
                    &Sha256::digest(&tbs_der),
                )
                .unwrap(),
            Key::P256(k) => {
                let s: p256::ecdsa::DerSignature = k.sign(&tbs_der);
                s.to_bytes().to_vec()
            }
            Key::P384(k) => {
                let s: p384::ecdsa::DerSignature = k.sign(&tbs_der);
                s.to_bytes().to_vec()
            }
        };
        let raw = RawCert {
            tbs,
            alg: sig_alg,
            sig: BitString::from_bytes(&sig).unwrap(),
        };
        s.cert = raw.to_der().unwrap();
        // Must round-trip through the strict production parser.
        cms::parse_certificate(&s.cert).unwrap();
        s
    }

    /// Sign a request the way a host signer would.
    fn sign(&self, req: &SignRequest<'_>) -> Vec<u8> {
        let digest = match req.input {
            SignerInput::SignedAttributes => hash(req.digest, req.data),
            SignerInput::Prehashed => req.data.to_vec(),
        };
        let mut rng = SysRng;
        match (&self.key, req.algorithm) {
            (Key::Rsa(k), SignatureAlgorithm::RsassaPss) => match req.digest {
                DigestAlgorithm::Sha256 => {
                    k.pss_sign_prehash_with_rng::<Sha256, _>(&mut rng, &digest)
                }
                DigestAlgorithm::Sha384 => {
                    k.pss_sign_prehash_with_rng::<Sha384, _>(&mut rng, &digest)
                }
                DigestAlgorithm::Sha512 => {
                    k.pss_sign_prehash_with_rng::<Sha512, _>(&mut rng, &digest)
                }
            }
            .unwrap(),
            (Key::Rsa(k), SignatureAlgorithm::RsaPkcs1v15) => match req.digest {
                DigestAlgorithm::Sha256 => k.sign(rsa::Pkcs1v15Sign::new::<Sha256>(), &digest),
                DigestAlgorithm::Sha384 => k.sign(rsa::Pkcs1v15Sign::new::<Sha384>(), &digest),
                DigestAlgorithm::Sha512 => k.sign(rsa::Pkcs1v15Sign::new::<Sha512>(), &digest),
            }
            .unwrap(),
            (Key::P256(k), _) => {
                let s: p256::ecdsa::DerSignature = k.sign_prehash(&digest).unwrap();
                s.to_bytes().to_vec()
            }
            (Key::P384(k), _) => {
                let s: p384::ecdsa::DerSignature = k.sign_prehash(&digest).unwrap();
                s.to_bytes().to_vec()
            }
            (k, a) => panic!(
                "test signer cannot produce {a:?} with {}",
                match k {
                    Key::Rsa(_) => "RSA",
                    Key::P256(_) => "P-256",
                    Key::P384(_) => "P-384",
                }
            ),
        }
    }

    fn identity(&self) -> SignerIdentity {
        SignerIdentity::Certificate(self.cert.clone())
    }

    fn complete_with(&self, p: &PreparedSignature) -> Completion {
        Completion::Signature {
            signature: self.sign(&p.request()),
            chain: vec![self.cert.clone()],
        }
    }
}

fn rsa_signer() -> &'static TestSigner {
    static S: OnceLock<TestSigner> = OnceLock::new();
    S.get_or_init(|| {
        let key = rsa::PrivateKey::new(&mut SysRng, 2048).unwrap();
        TestSigner::with_cert(Key::Rsa(Box::new(key)), "Nib Test RSA", 1)
    })
}

fn rsa_signer_other() -> &'static TestSigner {
    static S: OnceLock<TestSigner> = OnceLock::new();
    S.get_or_init(|| {
        let key = rsa::PrivateKey::new(&mut SysRng, 2048).unwrap();
        TestSigner::with_cert(Key::Rsa(Box::new(key)), "Nib Test RSA Other", 2)
    })
}

fn p256_signer() -> TestSigner {
    let key = p256::ecdsa::SigningKey::try_generate_from_rng(&mut SysRng).unwrap();
    TestSigner::with_cert(Key::P256(key), "Nib Test P-256", 3)
}

fn p384_signer() -> TestSigner {
    let key = p384::ecdsa::SigningKey::try_generate_from_rng(&mut SysRng).unwrap();
    TestSigner::with_cert(Key::P384(key), "Nib Test P-384", 4)
}

// ---------------------------------------------------------------- fixtures/oracles

fn fixture(name: &str) -> Vec<u8> {
    let p = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../nib-core/tests/fixtures")
        .join(name);
    std::fs::read(&p).unwrap_or_else(|e| panic!("missing fixture {}: {e}", p.display()))
}

fn open(name: &str) -> Writer {
    Writer::open(fixture(name)).unwrap().0
}

fn existing(name: &str) -> SigTarget {
    SigTarget::ExistingField(name.into())
}

fn new_field(name: &str, page: u32) -> SigTarget {
    SigTarget::NewInvisibleField {
        name: name.into(),
        page,
    }
}

/// The `/ByteRange` and DER CMS (trimmed of zero padding) of the signature
/// dictionary `/V` of field `field`.
fn extract(bytes: &[u8], field: &str) -> ([u64; 4], Vec<u8>, Vec<u8>) {
    let doc = Document::load_mem(bytes).unwrap();
    let mut found = None;
    for obj in doc.objects.values() {
        let Ok(d) = obj.as_dict() else { continue };
        if d.get(b"T").and_then(Object::as_str).ok() != Some(field.as_bytes()) {
            continue;
        }
        let v = d.get(b"V").and_then(Object::as_reference).unwrap();
        found = Some(doc.get_dictionary(v).unwrap().clone());
    }
    let sig = found.unwrap_or_else(|| panic!("no signed field {field}"));
    let br: Vec<u64> = sig
        .get(b"ByteRange")
        .unwrap()
        .as_array()
        .unwrap()
        .iter()
        .map(|o| o.as_i64().unwrap() as u64)
        .collect();
    let br: [u64; 4] = br.try_into().unwrap();
    let contents = sig.get(b"Contents").unwrap().as_str().unwrap();
    let der = contents[..der_prefix_len(contents).unwrap()].to_vec();
    let mut signed = bytes[br[0] as usize..(br[0] + br[1]) as usize].to_vec();
    signed.extend_from_slice(&bytes[br[2] as usize..(br[2] + br[3]) as usize]);
    (br, der, signed)
}

/// Required oracle: `openssl cms -verify -binary -inform DER -noverify`.
/// `NIB_OPENSSL` overrides the binary. Panics when OpenSSL is unavailable.
fn openssl_verify(der: &[u8], content: &[u8], tag: &str) {
    if let Err(e) = openssl_cms_verify(der, content, tag) {
        panic!("{tag}: openssl cms -verify failed\n{e}");
    }
}

fn openssl_cms_verify(der: &[u8], content: &[u8], tag: &str) -> Result<(), String> {
    let bin = std::env::var("NIB_OPENSSL").unwrap_or_else(|_| "openssl".into());
    let version = Command::new(&bin)
        .arg("version")
        .output()
        .unwrap_or_else(|e| {
            panic!("{tag}: required oracle `{bin}` is unavailable ({e}); install OpenSSL or set NIB_OPENSSL")
        });
    let v = String::from_utf8_lossy(&version.stdout).to_string();
    assert!(
        version.status.success() && v.starts_with("OpenSSL"),
        "{tag}: `{bin} version` reported {v:?}; OpenSSL 3+ is required (set NIB_OPENSSL)"
    );
    let dir = tempfile::tempdir().unwrap();
    let cms_path = dir.path().join("sig.der");
    let content_path = dir.path().join("content.bin");
    std::fs::write(&cms_path, der).unwrap();
    std::fs::write(&content_path, content).unwrap();
    let out = Command::new(&bin)
        .args([
            "cms",
            "-verify",
            "-binary",
            "-inform",
            "DER",
            "-noverify",
            "-in",
        ])
        .arg(&cms_path)
        .arg("-content")
        .arg(&content_path)
        .arg("-out")
        .arg(dir.path().join("out.bin"))
        .output()
        .unwrap();
    if out.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).into_owned())
    }
}

/// Required oracle: `qpdf --check`. Panics when qpdf is unavailable.
fn qpdf_check(bytes: &[u8], tag: &str) {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("out.pdf");
    std::fs::write(&p, bytes).unwrap();
    if let Ok(keep) = std::env::var("NIB_KEEP_PDFS") {
        let _ = std::fs::create_dir_all(&keep);
        let _ = std::fs::write(Path::new(&keep).join(format!("{tag}.pdf")), bytes);
    }
    let out = Command::new("qpdf")
        .arg("--check")
        .arg(&p)
        .output()
        .unwrap_or_else(|e| panic!("{tag}: required oracle qpdf is unavailable ({e})"));
    let code = out.status.code().unwrap_or(-1);
    assert!(
        code == 0 || code == 3,
        "{tag}: qpdf --check exited {code}\n{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
}

/// Every structural property of a completed signature, plus both oracles.
fn assert_signed(before: &[u8], after: &[u8], field: &str, hex_len: usize, tag: &str) {
    assert!(after.len() > before.len());
    assert_eq!(
        &after[..before.len()],
        before,
        "{tag}: earlier revision changed"
    );
    let (br, der, signed) = extract(after, field);
    let [a, b, c, d] = br;
    assert_eq!(a, 0);
    assert_eq!(after[b as usize], b'<', "{tag}: range must stop at '<'");
    assert_eq!(
        after[c as usize - 1],
        b'>',
        "{tag}: range must resume after '>'"
    );
    assert_eq!(
        c - b,
        hex_len as u64 + 2,
        "{tag}: gap is exactly the <...> token"
    );
    assert_eq!(c + d, after.len() as u64, "{tag}: range covers EOF");
    assert!(
        b as usize > before.len(),
        "{tag}: placeholder lies in the new revision"
    );
    let hex = &after[b as usize + 1..c as usize - 1];
    assert!(hex.iter().all(u8::is_ascii_hexdigit));
    assert!(hex[2 * der.len()..].iter().all(|&x| x == b'0'));
    openssl_verify(&der, &signed, tag);
    qpdf_check(after, tag);

    // The field reads back as occupied.
    let (_, _, fields) = Writer::open(after.to_vec()).unwrap();
    let f = fields.iter().find(|f| f.name == field).unwrap();
    assert_eq!(f.field_type, FieldType::Signature);
    assert_eq!(f.signature_state, Some(SignatureFieldState::Occupied));
}

fn sign_flow(
    name: &str,
    signer: &TestSigner,
    mut options: SignOptions,
    tag: &str,
) -> (Vec<u8>, Vec<u8>, Writer) {
    let before = fixture(name);
    let (mut w, _, _) = Writer::open(before.clone()).unwrap();
    options.signer = signer.identity();
    let field = match &options.target {
        SigTarget::ExistingField(n) => n.clone(),
        SigTarget::NewInvisibleField { name, .. } => name.clone(),
    };
    let hex = options.contents_hex_len;
    let p = prepare(&mut w, options).unwrap();
    assert!(w.signature_pending());
    assert_eq!(&*w.bytes(), &before[..], "prepare must not publish bytes");
    assert_eq!(w.revision(), 0);
    let out = p.complete(&mut w, signer.complete_with(&p)).unwrap();
    assert!(!w.signature_pending());
    assert_eq!(w.revision(), 1);
    assert_eq!(out.report.revision, 1);
    assert_eq!(&out.bytes[..], &w.bytes()[..]);
    assert_signed(&before, &out.bytes, &field, hex, tag);
    (before, out.bytes.to_vec(), w)
}

fn opts(target: SigTarget, algorithm: SignatureAlgorithm) -> SignOptions {
    // Identity is replaced by sign_flow / the caller.
    SignOptions::new(
        target,
        SignerIdentity::Sha256Fingerprint([0; 32]),
        algorithm,
    )
}

// ---------------------------------------------------------------- happy paths

#[test]
fn cades_rsa_pss_existing_field() {
    let mut o = opts(existing("Signature1"), SignatureAlgorithm::RsassaPss);
    o.claimed_time = Some(chrono::TimeZone::with_ymd_and_hms(&Utc, 2026, 1, 2, 3, 4, 5).unwrap());
    o.reason = Some("approval".into());
    o.name = Some("Nib Test".into());
    let (_, after, _) = sign_flow("sig-field.pdf", rsa_signer(), o, "cades_rsa_pss_existing");
    let s = String::from_utf8_lossy(&after);
    assert!(s.contains("/SubFilter/ETSI.CAdES.detached"));
    assert!(s.contains("/M(D:20260102030405Z)"));
    // No CAdES signingTime attribute.
    let (_, der, _) = extract(&after, "Signature1");
    let ci = ContentInfo::from_der(&der).unwrap();
    let sd: SignedData = ci.content.decode_as().unwrap();
    let si = sd.signer_infos.0.iter().next().unwrap();
    let attrs = si.signed_attrs.as_ref().unwrap();
    let oids: Vec<_> = attrs.iter().map(|a| a.oid).collect();
    assert_eq!(oids.len(), 3);
    assert!(!oids.contains(&rfc5911::ID_SIGNING_TIME));
    assert!(oids.contains(&rfc5911::ID_AA_SIGNING_CERTIFICATE_V_2));
    assert!(si.unsigned_attrs.is_none());
}

#[test]
fn pkcs7_rsa_pkcs1_new_field_on_plain_and_form_documents() {
    for (fixture_name, page) in [("text.pdf", 0), ("form.pdf", 0), ("incr-xrefstream.pdf", 1)] {
        let mut o = opts(new_field("Approval", page), SignatureAlgorithm::RsaPkcs1v15);
        o.sub_filter = SubFilter::AdbePkcs7Detached;
        o.digest = DigestAlgorithm::Sha512;
        let tag = format!("pkcs7_rsa_new_{fixture_name}");
        let (_, after, _) = sign_flow(fixture_name, rsa_signer(), o, &tag);
        let doc = Document::load_mem(&after).unwrap();
        let acro = doc
            .catalog()
            .unwrap()
            .get_deref(b"AcroForm", &doc)
            .unwrap()
            .as_dict()
            .unwrap();
        assert_eq!(acro.get(b"SigFlags").unwrap().as_i64().unwrap() & 3, 3);
        // Invisible, print + locked widget on the requested page.
        let page_id = doc.get_pages()[&(page + 1)];
        let annots = doc
            .get_dictionary(page_id)
            .unwrap()
            .get_deref(b"Annots", &doc)
            .unwrap()
            .as_array()
            .unwrap()
            .clone();
        let widget = annots
            .iter()
            .filter_map(|o| o.as_reference().ok())
            .map(|r| doc.get_dictionary(r).unwrap())
            .find(|d| d.get(b"T").and_then(Object::as_str).ok() == Some(b"Approval"))
            .expect("widget on page");
        assert_eq!(widget.get(b"F").unwrap().as_i64().unwrap(), 132);
        let rect: Vec<f32> = widget
            .get(b"Rect")
            .unwrap()
            .as_array()
            .unwrap()
            .iter()
            .map(|o| o.as_float().unwrap())
            .collect();
        assert_eq!(rect, vec![0.0; 4]);
    }
}

#[test]
fn cades_p256_prehashed_and_p384() {
    let p = p256_signer();
    let mut o = opts(existing("Signature1"), SignatureAlgorithm::EcdsaP256);
    o.input = SignerInput::Prehashed;
    sign_flow("sig-field.pdf", &p, o, "cades_p256_prehashed");

    let q = p384_signer();
    let o = opts(new_field("P384", 0), SignatureAlgorithm::EcdsaP384);
    assert_eq!(o.digest, DigestAlgorithm::Sha384);
    sign_flow("text.pdf", &q, o, "cades_p384");
}

#[test]
fn prehashed_request_is_the_digest_of_signed_attributes() {
    let s = p256_signer();
    let mut w = open("sig-field.pdf");
    let mut o = opts(existing("Signature1"), SignatureAlgorithm::EcdsaP256);
    o.signer = s.identity();
    let attrs = prepare(&mut w, o.clone()).unwrap();
    assert_eq!(attrs.request().data[0], 0x31, "SET OF tag");
    o.input = SignerInput::Prehashed;
    let pre = prepare(&mut w, o).unwrap();
    assert_eq!(pre.request().data, &Sha256::digest(&pre.signed_attrs)[..]);
    assert_eq!(pre.request().input, SignerInput::Prehashed);
}

#[test]
fn fingerprint_identity_and_complete_cms() {
    let s = rsa_signer();
    let before = fixture("sig-field.pdf");
    let (mut w, _, _) = Writer::open(before.clone()).unwrap();
    let mut o = opts(existing("Signature1"), SignatureAlgorithm::RsassaPss);
    o.signer = SignerIdentity::Sha256Fingerprint(Sha256::digest(&s.cert).into());
    let p = prepare(&mut w, o).unwrap();
    let cert = cms::parse_certificate(&s.cert).unwrap();
    let der = cms::build(&p.scheme, &p.signed_attrs, &s.sign(&p.request()), &[cert]).unwrap();
    let out = p.complete(&mut w, Completion::Cms(der)).unwrap();
    assert_signed(
        &before,
        &out.bytes,
        "Signature1",
        DEFAULT_CONTENTS_HEX_LEN,
        "complete_cms",
    );
}

#[test]
fn second_signature_and_presigned_base_keep_earlier_revisions() {
    let s = rsa_signer();
    let (first_before, first, mut w) = sign_flow(
        "text.pdf",
        s,
        opts(new_field("First", 0), SignatureAlgorithm::RsassaPss),
        "first_of_two",
    );
    let mut o = opts(new_field("Second", 0), SignatureAlgorithm::RsassaPss);
    o.signer = s.identity();
    let p = prepare(&mut w, o).unwrap();
    let out = p.complete(&mut w, s.complete_with(&p)).unwrap();
    assert_eq!(out.report.revision, 2);
    assert_signed(
        &first,
        &out.bytes,
        "Second",
        DEFAULT_CONTENTS_HEX_LEN,
        "second_of_two",
    );
    assert_eq!(&out.bytes[..first_before.len()], &first_before[..]);
    // The first signature still covers exactly its own revision.
    let (br, der, signed) = extract(&out.bytes, "First");
    assert_eq!(br[2] + br[3], first.len() as u64);
    openssl_verify(&der, &signed, "first_after_second");

    sign_flow(
        "incr-presigned.pdf",
        s,
        opts(new_field("Countersign", 0), SignatureAlgorithm::RsassaPss),
        "on_presigned",
    );
}

#[test]
fn openssl_oracle_detects_tampering() {
    let (_, after, _) = sign_flow(
        "sig-field.pdf",
        rsa_signer(),
        opts(existing("Signature1"), SignatureAlgorithm::RsassaPss),
        "oracle_tamper",
    );
    let (_, der, mut signed) = extract(&after, "Signature1");
    openssl_cms_verify(&der, &signed, "oracle_ok").unwrap();
    signed[100] ^= 1;
    assert!(openssl_cms_verify(&der, &signed, "oracle_tampered").is_err());
}

#[test]
fn custom_placeholder_size_is_exact() {
    let s = p256_signer();
    let mut o = opts(existing("Signature1"), SignatureAlgorithm::EcdsaP256);
    o.contents_hex_len = 4096;
    sign_flow("sig-field.pdf", &s, o, "custom_size");
}

// ---------------------------------------------------------------- rejections

fn assert_untouched(w: &Writer, before: &[u8]) {
    assert!(!w.signature_pending());
    assert_eq!(&*w.bytes(), before);
    assert_eq!(w.revision(), 0);
}

#[test]
fn rejects_bad_targets_without_staging() {
    let s = rsa_signer();
    let cases: Vec<(&str, SigTarget)> = vec![
        ("sig-field.pdf", existing("Missing")),
        ("sig-field.pdf", new_field("Signature1", 0)),
        ("sig-field.pdf", new_field("a.b", 0)),
        ("sig-field.pdf", new_field("", 0)),
        ("sig-field.pdf", new_field("Fresh", 7)),
        ("form.pdf", existing("name")),
        ("form.pdf", new_field("bottom", 0)),
        ("incr-presigned.pdf", existing("Signature1")),
    ];
    for (f, target) in cases {
        let before = fixture(f);
        let (mut w, _, _) = Writer::open(before.clone()).unwrap();
        let mut o = opts(target.clone(), SignatureAlgorithm::RsassaPss);
        o.signer = s.identity();
        let e = prepare(&mut w, o).unwrap_err();
        assert!(
            matches!(e, SignError::Writer(_)),
            "{f} {target:?}: unexpected {e:?}"
        );
        assert_untouched(&w, &before);
    }
}

#[test]
fn rejects_read_only_signature_field() {
    let mut doc = Document::load_mem(&fixture("sig-field.pdf")).unwrap();
    let id = doc
        .objects
        .iter()
        .find(|(_, o)| {
            o.as_dict()
                .is_ok_and(|d| d.get(b"FT").and_then(Object::as_name).ok() == Some(b"Sig"))
        })
        .map(|(id, _)| *id)
        .unwrap();
    doc.get_dictionary_mut(id).unwrap().set("Ff", 1);
    let mut bytes = Vec::new();
    doc.save_to(&mut bytes).unwrap();
    let (mut w, _, _) = Writer::open(bytes.clone()).unwrap();
    let mut o = opts(existing("Signature1"), SignatureAlgorithm::RsassaPss);
    o.signer = rsa_signer().identity();
    let e = prepare(&mut w, o).unwrap_err();
    assert!(format!("{e}").contains("read-only"), "{e}");
    assert_untouched(&w, &bytes);
}

#[test]
fn rejects_placeholder_collisions_in_text_entries() {
    for reason in ["see /Contents <00>", "/ByteRange [0 1 2 3]", "x endobj y"] {
        let before = fixture("sig-field.pdf");
        let (mut w, _, _) = Writer::open(before.clone()).unwrap();
        let mut o = opts(existing("Signature1"), SignatureAlgorithm::RsassaPss);
        o.signer = rsa_signer().identity();
        o.reason = Some(reason.into());
        let e = prepare(&mut w, o).unwrap_err();
        assert!(matches!(e, SignError::Placeholder(_)), "{reason}: {e:?}");
        assert_untouched(&w, &before);
    }
}

#[test]
fn rejects_odd_or_out_of_range_sizes() {
    for hex in [0, 1, 16_383, MAX_CONTENTS_HEX_LEN + 2] {
        let before = fixture("sig-field.pdf");
        let (mut w, _, _) = Writer::open(before.clone()).unwrap();
        let mut o = opts(existing("Signature1"), SignatureAlgorithm::EcdsaP256);
        o.contents_hex_len = hex;
        let e = prepare(&mut w, o).unwrap_err();
        assert!(matches!(e, SignError::Placeholder(_)), "{hex}: {e:?}");
        assert_untouched(&w, &before);
    }
}

#[test]
fn placeholder_too_small_reports_hex_chars_and_rolls_back() {
    let s = rsa_signer();
    let before = fixture("sig-field.pdf");
    let (mut w, _, _) = Writer::open(before.clone()).unwrap();
    let mut o = opts(existing("Signature1"), SignatureAlgorithm::RsassaPss);
    o.signer = s.identity();
    o.contents_hex_len = 512;
    let p = prepare(&mut w, o).unwrap();
    let sig = s.sign(&p.request());
    let cert = cms::parse_certificate(&s.cert).unwrap();
    let der_len = cms::build(&p.scheme, &p.signed_attrs, &sig, &[cert])
        .unwrap()
        .len();
    let e = p
        .complete(
            &mut w,
            Completion::Signature {
                signature: sig,
                chain: vec![s.cert.clone()],
            },
        )
        .unwrap_err();
    match e {
        SignError::PlaceholderTooSmall { required } => assert_eq!(required, 2 * der_len),
        e => panic!("{e:?}"),
    }
    assert!(
        w.signature_pending(),
        "failure keeps the prepared state retryable"
    );
    assert_eq!(&*w.bytes(), &before[..]);
    p.abort(&mut w);
    assert_untouched(&w, &before);
    // The writer is fully usable afterwards.
    let mut o = opts(existing("Signature1"), SignatureAlgorithm::RsassaPss);
    o.signer = s.identity();
    let p = prepare(&mut w, o).unwrap();
    let out = p.complete(&mut w, s.complete_with(&p)).unwrap();
    assert_signed(
        &before,
        &out.bytes,
        "Signature1",
        DEFAULT_CONTENTS_HEX_LEN,
        "after_too_small",
    );
}

#[test]
fn rejects_unsupported_combinations_and_levels() {
    let combos = [
        (
            SignatureAlgorithm::EcdsaP256,
            DigestAlgorithm::Sha384,
            SigLevel::BB,
        ),
        (
            SignatureAlgorithm::EcdsaP256,
            DigestAlgorithm::Sha512,
            SigLevel::BB,
        ),
        (
            SignatureAlgorithm::EcdsaP384,
            DigestAlgorithm::Sha256,
            SigLevel::BB,
        ),
    ];
    for (a, d, l) in combos {
        let before = fixture("sig-field.pdf");
        let (mut w, _, _) = Writer::open(before.clone()).unwrap();
        let mut o = opts(existing("Signature1"), a);
        o.digest = d;
        o.level = l;
        let e = prepare(&mut w, o).unwrap_err();
        assert!(
            matches!(e, SignError::UnsupportedAlgorithm(_)),
            "{a:?}/{d:?}: {e:?}"
        );
        assert_untouched(&w, &before);
    }
    for sf in [SubFilter::EtsiCadesDetached, SubFilter::AdbePkcs7Detached] {
        let mut w = open("sig-field.pdf");
        let mut o = opts(existing("Signature1"), SignatureAlgorithm::RsassaPss);
        o.sub_filter = sf;
        o.level = SigLevel::BT;
        let e = prepare(&mut w, o).unwrap_err();
        assert!(
            matches!(e, SignError::UnsupportedLevel(SigLevel::BT)),
            "{e:?}"
        );
        assert!(!w.signature_pending());
    }
}

#[test]
fn rejects_certificate_key_type_mismatch() {
    let p = p256_signer();
    let mut w = open("sig-field.pdf");
    let mut o = opts(existing("Signature1"), SignatureAlgorithm::RsassaPss);
    o.signer = p.identity();
    assert!(matches!(
        prepare(&mut w, o).unwrap_err(),
        SignError::Certificate(_)
    ));
    let mut o = opts(existing("Signature1"), SignatureAlgorithm::EcdsaP384);
    o.signer = p.identity();
    assert!(matches!(
        prepare(&mut w, o).unwrap_err(),
        SignError::Certificate(_)
    ));
    let mut o = opts(existing("Signature1"), SignatureAlgorithm::EcdsaP256);
    o.signer = SignerIdentity::Certificate(vec![0x30, 0x00]);
    assert!(matches!(
        prepare(&mut w, o).unwrap_err(),
        SignError::Certificate(_)
    ));
    assert!(!w.signature_pending());
}

/// Prepare an RSA-PSS CAdES signature on `sig-field.pdf`.
fn prepared() -> (Vec<u8>, Writer, PreparedSignature) {
    let before = fixture("sig-field.pdf");
    let (mut w, _, _) = Writer::open(before.clone()).unwrap();
    let mut o = opts(existing("Signature1"), SignatureAlgorithm::RsassaPss);
    o.signer = rsa_signer().identity();
    let p = prepare(&mut w, o).unwrap();
    (before, w, p)
}

fn assert_rejected_then_recoverable(
    before: &[u8],
    w: &mut Writer,
    p: &PreparedSignature,
    c: Completion,
    check: impl Fn(&SignError) -> bool,
) {
    let e = p.complete(w, c).unwrap_err();
    assert!(check(&e), "unexpected {e:?}");
    assert!(w.signature_pending());
    assert_eq!(&*w.bytes(), before);
    assert_eq!(w.revision(), 0);
}

#[test]
fn rejects_incorrect_signatures_and_certificates() {
    let s = rsa_signer();
    let (before, mut w, p) = prepared();
    let good = s.sign(&p.request());

    let mut bad = good.clone();
    bad[10] ^= 1;
    assert_rejected_then_recoverable(
        &before,
        &mut w,
        &p,
        Completion::Signature {
            signature: bad,
            chain: vec![s.cert.clone()],
        },
        |e| matches!(e, SignError::InvalidSignature(_)),
    );
    // Signature by another key with the expected certificate.
    let other = rsa_signer_other();
    assert_rejected_then_recoverable(
        &before,
        &mut w,
        &p,
        Completion::Signature {
            signature: other.sign(&p.request()),
            chain: vec![s.cert.clone()],
        },
        |e| matches!(e, SignError::InvalidSignature(_)),
    );
    // A different leaf than the one fixed at prepare.
    assert_rejected_then_recoverable(
        &before,
        &mut w,
        &p,
        Completion::Signature {
            signature: other.sign(&p.request()),
            chain: vec![other.cert.clone()],
        },
        |e| matches!(e, SignError::CertificateMismatch(_)),
    );
    assert_rejected_then_recoverable(
        &before,
        &mut w,
        &p,
        Completion::Signature {
            signature: good.clone(),
            chain: vec![],
        },
        |e| matches!(e, SignError::Certificate(_)),
    );
    // Signing the PDF digest instead of the signed attributes.
    let mut wrong_input = p.request();
    let pdf_digest = p.pdf_digest.clone();
    wrong_input.data = &pdf_digest;
    wrong_input.input = SignerInput::Prehashed;
    assert_rejected_then_recoverable(
        &before,
        &mut w,
        &p,
        Completion::Signature {
            signature: s.sign(&wrong_input),
            chain: vec![s.cert.clone()],
        },
        |e| matches!(e, SignError::InvalidSignature(_)),
    );
    // A complete CMS from the wrong signer.
    let other_cert = cms::parse_certificate(&other.cert).unwrap();
    let der = cms::build(
        &p.scheme,
        &p.signed_attrs,
        &other.sign(&p.request()),
        &[other_cert],
    )
    .unwrap();
    assert_rejected_then_recoverable(&before, &mut w, &p, Completion::Cms(der), |e| {
        matches!(e, SignError::CertificateMismatch(_))
    });

    // Still completes with the right signature afterwards.
    let out = p
        .complete(
            &mut w,
            Completion::Signature {
                signature: good,
                chain: vec![s.cert.clone()],
            },
        )
        .unwrap();
    assert_signed(
        &before,
        &out.bytes,
        "Signature1",
        DEFAULT_CONTENTS_HEX_LEN,
        "after_rejections",
    );
}

fn attr(oid: const_oid::ObjectIdentifier, value: Any) -> Attribute {
    Attribute {
        oid,
        values: SetOfVec::try_from(vec![value]).unwrap(),
    }
}

/// Signed attributes with the given (oid, value) list, signed by `s`, as a
/// complete CMS.
fn cms_with_attrs(p: &PreparedSignature, s: &TestSigner, attrs: Vec<Attribute>) -> Vec<u8> {
    let attrs_der = SetOfVec::try_from(attrs).unwrap().to_der().unwrap();
    let req = SignRequest {
        data: &attrs_der,
        input: SignerInput::SignedAttributes,
        digest: p.scheme.digest,
        algorithm: p.scheme.algorithm,
    };
    let cert = cms::parse_certificate(&s.cert).unwrap();
    cms::build(&p.scheme, &attrs_der, &s.sign(&req), &[cert]).unwrap()
}

fn base_attrs(p: &PreparedSignature) -> Vec<Attribute> {
    let a = x509_cert::attr::Attributes::from_der(&p.signed_attrs).unwrap();
    a.iter().cloned().collect()
}

#[test]
fn rejects_supplied_cms_violations() {
    let s = rsa_signer();
    let (before, mut w, p) = prepared();
    let invalid = |e: &SignError| matches!(e, SignError::InvalidCms(_));

    // Wrong message digest.
    let mut attrs = base_attrs(&p);
    for a in &mut attrs {
        if a.oid == rfc5911::ID_MESSAGE_DIGEST {
            *a = attr(
                rfc5911::ID_MESSAGE_DIGEST,
                Any::encode_from(&OctetString::new(vec![0u8; 32]).unwrap()).unwrap(),
            );
        }
    }
    let c = cms_with_attrs(&p, s, attrs);
    assert_rejected_then_recoverable(&before, &mut w, &p, Completion::Cms(c), invalid);

    // CAdES must not carry signingTime.
    let mut attrs = base_attrs(&p);
    let t = UtcTime::from_unix_duration(std::time::Duration::from_secs(1_700_000_000)).unwrap();
    attrs.push(attr(
        rfc5911::ID_SIGNING_TIME,
        Any::encode_from(&t).unwrap(),
    ));
    let c = cms_with_attrs(&p, s, attrs);
    assert_rejected_then_recoverable(&before, &mut w, &p, Completion::Cms(c), invalid);

    // Missing ESS signing-certificate-v2.
    let attrs: Vec<_> = base_attrs(&p)
        .into_iter()
        .filter(|a| a.oid != rfc5911::ID_AA_SIGNING_CERTIFICATE_V_2)
        .collect();
    let c = cms_with_attrs(&p, s, attrs);
    assert_rejected_then_recoverable(&before, &mut w, &p, Completion::Cms(c), invalid);

    // Wrong content type.
    let mut attrs = base_attrs(&p);
    for a in &mut attrs {
        if a.oid == rfc5911::ID_CONTENT_TYPE {
            *a = attr(
                rfc5911::ID_CONTENT_TYPE,
                Any::encode_from(&rfc5911::ID_SIGNED_DATA).unwrap(),
            );
        }
    }
    let c = cms_with_attrs(&p, s, attrs);
    assert_rejected_then_recoverable(&before, &mut w, &p, Completion::Cms(c), invalid);

    let cert = cms::parse_certificate(&s.cert).unwrap();
    let good = cms::build(
        &p.scheme,
        &p.signed_attrs,
        &s.sign(&p.request()),
        std::slice::from_ref(&cert),
    )
    .unwrap();

    // Tampered signature inside an otherwise valid CMS.
    let mut sig = s.sign(&p.request());
    sig[0] ^= 0x80;
    let c = cms::build(
        &p.scheme,
        &p.signed_attrs,
        &sig,
        std::slice::from_ref(&cert),
    )
    .unwrap();
    assert_rejected_then_recoverable(&before, &mut w, &p, Completion::Cms(c), |e| {
        matches!(e, SignError::InvalidSignature(_))
    });

    // Trailing garbage / truncation.
    let mut c = good.clone();
    c.push(0);
    assert_rejected_then_recoverable(&before, &mut w, &p, Completion::Cms(c), invalid);
    let c = good[..good.len() - 1].to_vec();
    assert_rejected_then_recoverable(&before, &mut w, &p, Completion::Cms(c), invalid);

    // Unsigned attributes: a timestamp token is B-T (unsupported), anything
    // else is refused.
    let with_unsigned = |oid| {
        let ci = ContentInfo::from_der(&good).unwrap();
        let mut sd: SignedData = ci.content.decode_as().unwrap();
        let mut si = sd.signer_infos.0.iter().next().unwrap().clone();
        si.unsigned_attrs = Some(
            SetOfVec::try_from(vec![attr(
                oid,
                Any::encode_from(&OctetString::new(vec![1]).unwrap()).unwrap(),
            )])
            .unwrap(),
        );
        sd.signer_infos = SignerInfos::try_from(vec![si]).unwrap();
        ContentInfo {
            content_type: ci.content_type,
            content: Any::encode_from(&sd).unwrap(),
        }
        .to_der()
        .unwrap()
    };
    let c = with_unsigned(algorithm::ID_AA_TIME_STAMP_TOKEN);
    assert_rejected_then_recoverable(&before, &mut w, &p, Completion::Cms(c), |e| {
        matches!(e, SignError::UnsupportedLevel(SigLevel::BT))
    });
    let c = with_unsigned(rfc5911::ID_CONTENT_TYPE);
    assert_rejected_then_recoverable(&before, &mut w, &p, Completion::Cms(c), invalid);

    // Wrong digest algorithm in the SignerInfo.
    let mut wrong = p.scheme;
    wrong.digest = DigestAlgorithm::Sha384;
    let c = cms::build(&wrong, &p.signed_attrs, &s.sign(&p.request()), &[cert]).unwrap();
    assert_rejected_then_recoverable(&before, &mut w, &p, Completion::Cms(c), |e| {
        matches!(
            e,
            SignError::InvalidCms(_) | SignError::UnsupportedAlgorithm(_)
        )
    });

    let out = p.complete(&mut w, Completion::Cms(good)).unwrap();
    assert_signed(
        &before,
        &out.bytes,
        "Signature1",
        DEFAULT_CONTENTS_HEX_LEN,
        "after_cms_violations",
    );
}

#[test]
fn pkcs7_requires_ess_signing_certificate_v2() {
    let s = rsa_signer();
    let before = fixture("sig-field.pdf");
    let (mut w, _, _) = Writer::open(before.clone()).unwrap();
    let mut o = opts(existing("Signature1"), SignatureAlgorithm::RsaPkcs1v15);
    o.sub_filter = SubFilter::AdbePkcs7Detached;
    o.signer = s.identity();
    let p = prepare(&mut w, o).unwrap();
    assert!(base_attrs(&p)
        .iter()
        .any(|a| a.oid == rfc5911::ID_AA_SIGNING_CERTIFICATE_V_2));
    let attrs: Vec<_> = base_attrs(&p)
        .into_iter()
        .filter(|a| a.oid != rfc5911::ID_AA_SIGNING_CERTIFICATE_V_2)
        .collect();
    let c = cms_with_attrs(&p, s, attrs);
    assert_rejected_then_recoverable(
        &before,
        &mut w,
        &p,
        Completion::Cms(c),
        |e| matches!(e, SignError::InvalidCms(m) if m.contains("signing-certificate-v2")),
    );
    let out = p.complete(&mut w, s.complete_with(&p)).unwrap();
    assert_signed(
        &before,
        &out.bytes,
        "Signature1",
        DEFAULT_CONTENTS_HEX_LEN,
        "pkcs7_with_ess",
    );
}

#[test]
fn stale_prepared_signature_is_refused() {
    let s = rsa_signer();
    let before = fixture("text.pdf");
    let (mut w, _, _) = Writer::open(before.clone()).unwrap();
    let mut o = opts(new_field("A", 0), SignatureAlgorithm::RsassaPss);
    o.signer = s.identity();
    let first = prepare(&mut w, o.clone()).unwrap();
    o.target = new_field("B", 0);
    let second = prepare(&mut w, o).unwrap();
    let e = first.complete(&mut w, s.complete_with(&first)).unwrap_err();
    assert!(
        matches!(e, SignError::Writer(WriterError::StaleSignature(_))),
        "{e:?}"
    );
    // Aborting a stale preparation must not drop the current one.
    first.abort(&mut w);
    assert!(w.signature_pending());
    let out = second.complete(&mut w, s.complete_with(&second)).unwrap();
    assert_signed(
        &before,
        &out.bytes,
        "B",
        DEFAULT_CONTENTS_HEX_LEN,
        "stale_then_second",
    );
    // A completed preparation cannot be adopted twice.
    let e = second
        .complete(&mut w, s.complete_with(&second))
        .unwrap_err();
    assert!(
        matches!(e, SignError::Writer(WriterError::StaleSignature(_))),
        "{e:?}"
    );
    assert_eq!(w.revision(), 1);
}

#[test]
fn rewrite_and_flatten_refused_while_pending_and_after_signing() {
    use nib_core::ap::{NoAssets, NoFonts};
    use nib_core::delta::Model;
    use nib_core::writer::FlushOptions;
    let s = rsa_signer();
    let (_, mut w, p) = prepared();
    let model = Model {
        document_id: "signed".into(),
        page_count: w.page_count(),
        revision: 0,
        order: Vec::new(),
        annotations: Default::default(),
        form_fields: Default::default(),
        signatures: Default::default(),
        signed_barrier: Default::default(),
    };
    for o in [
        FlushOptions::full_rewrite(),
        FlushOptions::incremental().flatten(true),
    ] {
        let e = w.flush(&[], &model, o, &NoAssets, &NoFonts).unwrap_err();
        assert!(matches!(e, WriterError::SignaturePending), "{e:?}");
    }
    p.complete(&mut w, s.complete_with(&p)).unwrap();
    for o in [
        FlushOptions::full_rewrite(),
        FlushOptions::incremental().flatten(true),
    ] {
        let e = w.flush(&[], &model, o, &NoAssets, &NoFonts).unwrap_err();
        assert!(matches!(e, WriterError::SignedDocument), "{e:?}");
    }
}

// ---------------------------------------------------------------- placeholder units

const OBJ: &[u8] = b"%base\n5 0 obj\n<</Type/Sig/ByteRange[0 9999999999 9999999999 9999999999]/Contents<00000000>/Reason(ok)>>\nendobj\ntrailer\n";

fn locate_variant(body: &[u8], hex: usize) -> Result<Placeholder, SignError> {
    placeholder::locate(body, 6, (5, 0), hex)
}

#[test]
fn placeholder_locates_exact_offsets() {
    let ph = locate_variant(OBJ, 8).unwrap();
    assert_eq!(OBJ[ph.byte_range_open], b'[');
    assert_eq!(OBJ[ph.byte_range_close], b']');
    assert_eq!(OBJ[ph.contents_open], b'<');
    assert_eq!(OBJ[ph.contents_end - 1], b'>');
    assert_eq!(ph.hex_len(), 8);
    let mut bytes = OBJ.to_vec();
    let br = ph.byte_range(bytes.len()).unwrap();
    placeholder::patch_byte_range(&mut bytes, &ph, br).unwrap();
    assert_eq!(bytes.len(), OBJ.len());
    placeholder::check_byte_range(br, &ph, bytes.len()).unwrap();
    // Re-locating after patching fails: the range is no longer a placeholder.
    assert!(locate_variant(&bytes, 8).is_err());
    placeholder::embed_contents(&mut bytes, &ph, &[0xAB, 0x01]).unwrap();
    assert_eq!(&bytes[ph.contents_open..ph.contents_end], b"<AB010000>");
    assert!(matches!(
        placeholder::embed_contents(&mut bytes, &ph, &[1, 2, 3, 4, 5]),
        Err(SignError::PlaceholderTooSmall { required: 10 })
    ));
}

#[test]
fn placeholder_rejects_malformed_and_ambiguous_variants() {
    let s = String::from_utf8(OBJ.to_vec()).unwrap();
    let variants = [
        ("missing object", s.replace("5 0 obj", "6 0 obj"), 8),
        (
            "object twice",
            s.replace("trailer", "5 0 obj\n<<>>\nendobj\ntrailer"),
            8,
        ),
        ("missing endobj", s.replace("endobj", "endob"), 8),
        ("no /ByteRange", s.replace("/ByteRange", "/ByteRangeX"), 8),
        (
            "two /Contents",
            s.replace("/Reason(ok)", "/Contents<00>"),
            8,
        ),
        ("/Contents in a string", s.replace("(ok)", "(/Contents)"), 8),
        (
            "/ByteRange in another object",
            s.replace("trailer", "7 0 obj\n<</ByteRange[]>>\nendobj\ntrailer"),
            8,
        ),
        (
            "range not array",
            s.replace("/ByteRange[", "/ByteRange 1["),
            8,
        ),
        (
            "range not closed",
            s.replace("9999999999]", "9999999999"),
            8,
        ),
        (
            "range wrong values",
            s.replace("[0 9999999999", "[1 9999999999"),
            8,
        ),
        ("range 3 values", s.replace(" 9999999999]", "]"), 8),
        (
            "contents dict",
            s.replace("/Contents<00000000>", "/Contents<<>>"),
            8,
        ),
        ("contents literal", s.replace("<00000000>", "(00000000)"), 8),
        ("contents odd", s.replace("<00000000>", "<0000000>"), 7),
        ("contents nonzero", s.replace("<00000000>", "<00000001>"), 8),
        (
            "contents unclosed",
            s.replace("<00000000>/Reason(ok)>>\nendobj\ntrailer\n", "<0000"),
            8,
        ),
        ("wrong expected length", s.clone(), 10),
    ];
    for (what, body, hex) in variants {
        let r = locate_variant(body.as_bytes(), hex);
        assert!(matches!(r, Err(SignError::Placeholder(_))), "{what}: {r:?}");
    }
    // Object entirely within the base revision.
    assert!(placeholder::locate(OBJ, OBJ.len(), (5, 0), 8).is_err());
    assert!(placeholder::locate(OBJ, OBJ.len() - 10, (5, 0), 8).is_err());
}

#[test]
fn placeholder_rejects_offset_overflow_and_malformed_ranges() {
    let ph = locate_variant(OBJ, 8).unwrap();
    let mut bytes = OBJ.to_vec();
    assert!(matches!(
        placeholder::patch_byte_range(&mut bytes, &ph, [0, 10_000_000_000, 1, 1]),
        Err(SignError::OffsetOverflow(10_000_000_000))
    ));
    assert_eq!(bytes, OBJ, "a failed patch leaves the bytes untouched");
    let good = ph.byte_range(bytes.len()).unwrap();
    let len = bytes.len();
    for bad in [
        [1, good[1], good[2], good[3]],
        [0, good[1] + 1, good[2], good[3]],
        [0, good[1], good[2] - 1, good[3]],
        [0, good[1], good[2], good[3] + 1],
        [0, good[1], u64::MAX, 2],
    ] {
        assert!(
            matches!(
                placeholder::check_byte_range(bad, &ph, len),
                Err(SignError::Placeholder(_))
            ),
            "{bad:?}"
        );
    }
    assert!(ph.byte_range(ph.contents_end - 1).is_err());
}

#[test]
fn der_prefix_len_trims_padding_and_rejects_garbage() {
    let s = p256_signer();
    let (_, mut w, _) = prepared();
    let mut o = opts(existing("Signature1"), SignatureAlgorithm::EcdsaP256);
    o.signer = s.identity();
    let p = prepare(&mut w, o).unwrap();
    let cert = cms::parse_certificate(&s.cert).unwrap();
    let der = cms::build(&p.scheme, &p.signed_attrs, &s.sign(&p.request()), &[cert]).unwrap();
    let mut padded = der.clone();
    padded.extend_from_slice(&[0; 64]);
    assert_eq!(der_prefix_len(&padded).unwrap(), der.len());
    assert!(der_prefix_len(&der[..der.len() - 1]).is_err());
    assert!(der_prefix_len(&[0; 16]).is_err());
    assert!(der_prefix_len(&[]).is_err());
}
