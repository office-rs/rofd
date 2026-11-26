//! Certificate files (PKCS#12) and the certificate store: the path a
//! certificate the user imported takes into signing (spec 2026-11-23
//! §10.4, §10.5).

use base64::Engine;
use rofd_sign::{
    sm2_verify, CertificateEntry, CertificateId, CertificateStore, PfxSigner, SignError, Signer,
    DEFAULT_SM2_USER_ID,
};

fn fixture(name: &str) -> Vec<u8> {
    std::fs::read(format!("tests/fixtures/{name}"))
        .unwrap_or_else(|e| panic!("fixture {name}: {e}"))
}

fn is_certificate_error(result: Result<PfxSigner, SignError>) -> bool {
    matches!(result, Err(SignError::Certificate { .. }))
}

fn is_password_error(result: Result<PfxSigner, SignError>) -> bool {
    matches!(result, Err(SignError::Password { .. }))
}

#[test]
fn opens_ofdrw_test_p12() {
    let mut signer = PfxSigner::open(&fixture("USER.p12"), "777777").expect("open");
    let certificate = signer.certificate().expect("the certificate");
    assert_eq!(certificate.subject_cn, "Test Certificate");
    assert_eq!(
        certificate.issuer_cn,
        "OFD Reader and WriterROOT Certificate"
    );
    assert_eq!(certificate.serial_hex, "01719D2DAB60");
    assert_eq!(certificate.public_key_sec1.len(), 65);
    assert_eq!(certificate.public_key_sec1[0], 4);
    // The certificate the digital-signature sample was made with.
    assert_eq!(
        signer.certificate_der(),
        fixture("digital_signer_cert.der").as_slice()
    );
    let signature = signer.sign(b"message").expect("sign");
    assert!(sm2_verify(
        &certificate.public_key_sec1,
        DEFAULT_SM2_USER_ID,
        b"message",
        &signature
    ));
}

#[test]
fn opens_an_openssl_sm2_file_with_modern_protection() {
    let mut signer = PfxSigner::open(&fixture("sm2_test.p12"), "changeit").expect("open");
    let certificate = signer.certificate().expect("the certificate");
    assert_eq!(certificate.subject_cn, "rofd-sm2-test");
    assert_eq!(certificate.public_key_sec1.len(), 65);
    let signature = signer.sign(b"another message").expect("sign");
    assert!(sm2_verify(
        &certificate.public_key_sec1,
        DEFAULT_SM2_USER_ID,
        b"another message",
        &signature
    ));
}

#[test]
fn wrong_password_and_non_sm2_key_are_errors() {
    // A wrong password is its own error: a host must not tell a user whose
    // file cannot be read that the password is wrong (spec section 16).
    assert!(is_password_error(PfxSigner::open(
        &fixture("USER.p12"),
        "wrong"
    )));
    assert!(is_password_error(PfxSigner::open(&fixture("USER.p12"), "")));
    assert!(is_password_error(PfxSigner::open(
        &fixture("sm2_test.p12"),
        "wrong"
    )));
    assert!(
        is_certificate_error(PfxSigner::open(&fixture("rsa_test.p12"), "changeit")),
        "an RSA key is not an SM2 key"
    );
    assert!(is_certificate_error(PfxSigner::open(
        b"not a certificate file",
        "x"
    )));
    assert!(is_certificate_error(PfxSigner::open(&[], "x")));
    // Every failure says which of the two went wrong.
    let Err(error) = PfxSigner::open(&fixture("USER.p12"), "wrong") else {
        panic!("expected an error");
    };
    assert!(error.to_string().contains("password"), "{error}");
    let Err(error) = PfxSigner::open(&fixture("rsa_test.p12"), "changeit") else {
        panic!("expected an error");
    };
    assert!(error.to_string().contains("SM2"), "{error}");
}

#[test]
fn store_round_trips_through_bytes_without_private_key() {
    let mut store = CertificateStore::default();
    assert!(store.list().is_empty());

    let entry = store.import(fixture("USER.p12"), "777777").expect("import");
    assert_eq!(
        entry.label, "Test Certificate",
        "the subject names the entry by default"
    );
    assert_eq!(entry.subject_cn, "Test Certificate");
    assert!(!entry.id.0.is_empty());

    // The serialized store keeps each certificate file exactly as imported -
    // still encrypted - plus display data, and nothing else.
    let bytes = store.to_bytes().expect("to_bytes");
    let json: serde_json::Value = serde_json::from_slice(&bytes).expect("JSON");
    let items = json.as_array().expect("a list of entries");
    assert_eq!(items.len(), 1);
    let item = items[0].as_object().unwrap();
    let sorted: Vec<&str> = {
        let mut keys: Vec<&str> = item.keys().map(String::as_str).collect();
        keys.sort_unstable();
        keys
    };
    assert_eq!(
        sorted,
        vec![
            "file",
            "id",
            "issuer_cn",
            "label",
            "not_after",
            "not_before",
            "serial",
            "subject_cn"
        ],
        "no field carries key material"
    );
    let kept = item["file"].as_str().expect("the file as base64");
    assert_eq!(
        base64::engine::general_purpose::STANDARD
            .decode(kept)
            .expect("base64"),
        fixture("USER.p12"),
        "the file is stored byte for byte as imported"
    );

    let restored = CertificateStore::from_bytes(&bytes).expect("from_bytes");
    assert_eq!(restored.list(), store.list());
    let mut signer = restored.signer(&entry.id, "777777").expect("signer");
    let signature = signer.sign(b"m").expect("still signs after a round trip");
    assert!(sm2_verify(
        &signer.certificate().unwrap().public_key_sec1,
        DEFAULT_SM2_USER_ID,
        b"m",
        &signature
    ));
    assert!(is_password_error(restored.signer(&entry.id, "wrong")));
    assert!(is_certificate_error(
        restored.signer(&CertificateId("no-such-id".to_owned()), "777777")
    ));

    store.remove(&entry.id);
    assert!(store.list().is_empty());
    assert!(store.to_bytes().is_ok());
}

#[test]
fn importing_the_same_file_twice_keeps_one_entry() {
    let mut store = CertificateStore::default();
    let first = store.import(fixture("USER.p12"), "777777").unwrap();
    let second = store.import(fixture("USER.p12"), "777777").unwrap();
    assert_eq!(
        first.id, second.id,
        "the id is derived from the certificate"
    );
    assert_eq!(store.list().len(), 1);
    assert_eq!(store.list()[0], second);
}

#[test]
fn entries_are_listed_by_label() {
    let mut store = CertificateStore::default();
    store.import(fixture("USER.p12"), "777777").unwrap();
    store.import(fixture("sm2_test.p12"), "changeit").unwrap();
    store
        .import(fixture("rsa_test.p12"), "changeit")
        .unwrap_err();
    let listed = store.list();
    let labels: Vec<&str> = listed.iter().map(|e| e.label.as_str()).collect();
    assert_eq!(labels, vec!["rofd-sm2-test", "Test Certificate"]);
    assert!(store
        .list()
        .windows(2)
        .all(|pair| pair[0].label.to_lowercase() < pair[1].label.to_lowercase()));
}

#[test]
fn an_empty_store_round_trips() {
    let store = CertificateStore::default();
    let bytes = store.to_bytes().unwrap();
    assert_eq!(
        CertificateStore::from_bytes(&bytes).unwrap().list(),
        Vec::<CertificateEntry>::new()
    );
    assert!(CertificateStore::from_bytes(b"not json").is_err());
    assert!(CertificateStore::from_bytes(b"{}").is_err());
}

#[test]
fn the_key_s_certificate_is_chosen_among_several() {
    use keystore_core::{Certificate, KeyStoreFormat as _, Password};
    use keystore_pkcs12::{BagAttributes, CertBag, Pkcs12Store, SafeBag};
    // A file with a chain lists several certificates. Put one that is not
    // the key's in front of the key's own.
    let mut store =
        Pkcs12Store::parse(&fixture("sm2_test.p12"), &Password::new("changeit")).expect("parse");
    let stranger = rofd_sign::TestCa::from_seed("someone else", b"seed-x").cert_der;
    store.safes[0].bags.insert(
        0,
        SafeBag::Cert(CertBag::x509(BagAttributes::new(), Certificate(stranger))),
    );
    let bytes = store.encode(&Password::new("changeit")).expect("encode");

    let mut signer = PfxSigner::open(&bytes, "changeit").expect("open");
    assert_eq!(
        signer.certificate().expect("certificate").subject_cn,
        "rofd-sm2-test",
        "the certificate whose public key the file's key derives"
    );
    let public_key = signer.certificate().unwrap().public_key_sec1;
    let signature = signer.sign(b"m").expect("sign");
    assert!(sm2_verify(
        &public_key,
        DEFAULT_SM2_USER_ID,
        b"m",
        &signature
    ));
}

#[test]
fn an_imported_certificate_file_can_be_taken_back_out() {
    // A host keeps its own copies of what to sign with: the file, as
    // imported and still encrypted, is readable again by id.
    let mut store = CertificateStore::default();
    assert_eq!(
        store.certificate_file(&CertificateId("no-such-id".to_owned())),
        None
    );
    let entry = store.import(fixture("USER.p12"), "777777").unwrap();
    assert_eq!(
        store.certificate_file(&entry.id),
        Some(fixture("USER.p12").as_slice()),
        "byte for byte as imported"
    );
    store.remove(&entry.id);
    assert_eq!(store.certificate_file(&entry.id), None);
}
