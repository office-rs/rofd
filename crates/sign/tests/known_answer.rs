//! SES signature parsing and the SM2/SM3 wrappers against real signed
//! packages (the ofdrw samples in `tests/fixtures/`).

use rofd_io::zip_util::read_all_entries;
use rofd_sign::{parse_certificate, parse_ses_signature, sm2_verify, sm3, DEFAULT_SM2_USER_ID};

fn fixture(name: &str) -> Vec<u8> {
    std::fs::read(format!("tests/fixtures/{name}"))
        .unwrap_or_else(|e| panic!("fixture {name}: {e}"))
}

fn entry(bytes: &[u8], name: &str) -> Vec<u8> {
    read_all_entries(bytes)
        .expect("read entries")
        .into_iter()
        .find(|(n, _)| n == name)
        .unwrap_or_else(|| panic!("entry {name} present"))
        .1
}

fn signed_value(sample: &str) -> Vec<u8> {
    entry(&fixture(sample), "Doc_0/Signs/Sign_0/SignedValue.dat")
}

#[test]
fn sesv4_fixture_signature_verifies() {
    let package = fixture("SESV4SignDoc.ofd");
    let value = entry(&package, "Doc_0/Signs/Sign_0/SignedValue.dat");
    let ses = parse_ses_signature(&value).expect("SES v4");
    let certificate = parse_certificate(ses.cert_der).expect("the signer certificate");
    assert!(
        sm2_verify(
            &certificate.public_key_sec1,
            DEFAULT_SM2_USER_ID,
            ses.tbs_raw,
            ses.signature_der
        ),
        "the sample's signature verifies against its own certificate and TBS bytes"
    );
    // The signed data hash is SM3 of the signature description file.
    let description = entry(&package, "Doc_0/Signs/Sign_0/Signature.xml");
    assert_eq!(ses.data_hash, sm3(&description));
}

#[test]
fn sesv1_fixture_signature_verifies() {
    let package = fixture("SESV1SignDoc.ofd");
    let value = entry(&package, "Doc_0/Signs/Sign_0/SignedValue.dat");
    let ses = parse_ses_signature(&value).expect("SES v1");
    let certificate = parse_certificate(ses.cert_der).expect("the signer certificate");
    assert!(sm2_verify(
        &certificate.public_key_sec1,
        DEFAULT_SM2_USER_ID,
        ses.tbs_raw,
        ses.signature_der
    ));
}

#[test]
fn parse_ses_signature_reads_v4_and_v1() {
    for (sample, version) in [("SESV4SignDoc.ofd", 4), ("SESV1SignDoc.ofd", 1)] {
        let package = fixture(sample);
        let value = entry(&package, "Doc_0/Signs/Sign_0/SignedValue.dat");
        let ses = parse_ses_signature(&value).unwrap_or_else(|e| panic!("{sample}: {e}"));
        assert_eq!(ses.version, version);
        assert!(ses.tbs_raw.len() > 100, "TBS bytes are substantial");
        assert_eq!(
            ses.signature_der[0], 0x30,
            "the signature value is a DER SEQUENCE"
        );
        assert_eq!(ses.data_hash.len(), 32, "dataHash is an SM3 value");
        assert!(ses.time.is_some(), "{sample}: the signing time is read");
        // The seal embedded in the TBS equals the standalone Seal.esl.
        assert_eq!(
            ses.embedded_seal_raw,
            entry(&package, "Doc_0/Signs/Sign_0/Seal.esl").as_slice(),
            "{sample}: embedded seal matches Seal.esl"
        );
    }
}

#[test]
fn parse_ses_signature_rejects_other_layouts() {
    // SES v5 and the bare digital signature are not SES v1/v4 signatures.
    assert!(parse_ses_signature(&signed_value("SESV5SignDoc.ofd")).is_err());
    assert!(parse_ses_signature(&signed_value("DigitalSign.ofd")).is_err());
    assert!(parse_ses_signature(&[]).is_err());
}

#[test]
fn every_prefix_of_a_signature_value_is_ok_or_err() {
    for sample in ["SESV4SignDoc.ofd", "SESV1SignDoc.ofd"] {
        let value = signed_value(sample);
        for end in 0..=value.len() {
            let _ = parse_ses_signature(&value[..end]);
        }
    }
}
