//! DER reader and X.509 subset against real signed packages (the ofdrw
//! samples in `tests/fixtures/`).

use rofd_io::zip_util::read_all_entries;
use rofd_sign::{
    parse_certificate, parse_children, parse_one, DerWriter, TAG_INTEGER, TAG_OCTET_STRING,
    TAG_SEQUENCE,
};

const SAMPLES: [&str; 4] = [
    "SESV4SignDoc.ofd",
    "SESV1SignDoc.ofd",
    "SESV5SignDoc.ofd",
    "DigitalSign.ofd",
];

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
fn reads_version_from_each_fixture() {
    for (sample, version) in [
        ("SESV4SignDoc.ofd", 4),
        ("SESV1SignDoc.ofd", 1),
        ("SESV5SignDoc.ofd", 5),
    ] {
        let value = signed_value(sample);
        let outer = parse_one(&value).expect(sample);
        assert_eq!(outer.tag, TAG_SEQUENCE, "{sample}");
        let kids = parse_children(&outer).expect("outer children");
        let tbs = &kids[0];
        let tbs_kids = parse_children(tbs).expect("TBS children");
        assert_eq!(tbs_kids[0].as_i64().unwrap(), version, "{sample}");
    }
    // The digital signature sample is a bare r/s pair.
    let value = signed_value("DigitalSign.ofd");
    let outer = parse_one(&value).expect("digital sample");
    let kids = parse_children(&outer).expect("children");
    assert_eq!(
        (kids.len(), kids[0].tag, kids[1].tag),
        (2, TAG_INTEGER, TAG_INTEGER)
    );
}

#[test]
fn every_prefix_of_a_fixture_is_ok_or_err() {
    for sample in SAMPLES {
        let package = fixture(sample);
        for name in [
            "Doc_0/Signs/Sign_0/SignedValue.dat",
            "Doc_0/Signs/Sign_0/Seal.esl",
        ] {
            // A digital signature has no seal entry.
            let Some(bytes) = read_all_entries(&package)
                .expect("read entries")
                .into_iter()
                .find(|(n, _)| n == name)
                .map(|(_, bytes)| bytes)
            else {
                continue;
            };
            for end in 0..=bytes.len() {
                let _ = parse_one(&bytes[..end]);
            }
        }
    }
}

#[test]
fn parses_certificate_embedded_in_sesv4_fixture() {
    let value = signed_value("SESV4SignDoc.ofd");
    let root = parse_one(&value).expect("SES v4");
    // SES v4 puts the signer certificate in the OCTET STRING after the TBS
    // (the seal carries a second certificate, its maker's).
    let kids = parse_children(&root).expect("outer children");
    assert_eq!(kids[1].tag, TAG_OCTET_STRING);
    let cert = &parse_certificate(kids[1].content).expect("the signer certificate parses");
    assert_eq!(cert.public_key_sec1.len(), 65, "04 || X || Y");
    assert_eq!(cert.public_key_sec1[0], 4);
    assert!(!cert.subject_cn.is_empty());
    assert!(!cert.issuer_cn.is_empty());
    assert!(!cert.serial_hex.is_empty());
    assert!(cert.not_before.is_some());
    assert!(cert.not_after.is_some());
}

#[test]
fn a_non_certificate_is_rejected() {
    let mut writer = DerWriter::new();
    writer.sequence(|w| {
        w.integer(1);
    });
    assert!(parse_certificate(&writer.into_bytes()).is_err());
    assert!(parse_certificate(&[0]).is_err());
}

#[test]
fn builds_an_sm2_public_key_bit_string() {
    // SPKI-shaped output: the writer produces exactly the bytes the reader
    // expects for a public key (BIT STRING with no unused bits).
    let mut writer = DerWriter::new();
    writer.sequence(|w| {
        w.oid("1.2.840.10045.2.1");
        w.bit_string(&[4, 1, 2, 3]);
    });
    let bytes = writer.into_bytes();
    let node = parse_one(&bytes).unwrap();
    let kids = parse_children(&node).unwrap();
    assert_eq!(kids[1].as_bit_string().unwrap(), vec![4, 1, 2, 3]);
}
