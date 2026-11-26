//! The encoders: the signature description file, the SES v4 seal structures
//! and the GB/T 35275 signed message. Layouts are pinned to the structures
//! read back by the production reader and, end to end, by the verification
//! pipeline.

use rofd_dom::Rect;
use rofd_io::zip_util::{read_all_entries, write_zip};
use rofd_sign::{
    build_e_seal_info, build_signed_data, build_tbs, finish_seal, finish_signature,
    generalized_time, parse_certificate, parse_children, parse_one, parse_ses_signature,
    parse_signed_data, signature_xml, sm2_verify, sm3, verify, Reference, SealInput,
    SignatureOverall, SignatureXmlInput, Signer, StampRef, TestCa, VerifyOptions,
    DEFAULT_SM2_USER_ID, TAG_IA5_STRING, TAG_INTEGER, TAG_OCTET_STRING, TAG_SEQUENCE,
};

const NOW: &str = "2026-11-24 10:00:00";

fn ca() -> TestCa {
    TestCa::from_seed("rofd test signer", b"encode-seed")
}

fn tiny_png() -> Vec<u8> {
    let mut buf = std::io::Cursor::new(Vec::new());
    let img = image::RgbImage::from_raw(4, 2, [255, 0, 0].repeat(8)).unwrap();
    image::DynamicImage::ImageRgb8(img)
        .write_to(&mut buf, image::ImageFormat::Png)
        .unwrap();
    buf.into_inner()
}

/// The tags of a node's children.
fn child_tags(node: &rofd_sign::DerNode<'_>) -> Vec<u8> {
    parse_children(node)
        .unwrap()
        .iter()
        .map(|kid| kid.tag)
        .collect()
}

// ---- Signature.xml ----

fn stamps() -> Vec<StampRef<'static>> {
    vec![StampRef {
        id: 2,
        page_id: "1",
        boundary: Rect {
            x: 50.0,
            y: 50.0,
            w: 40.0,
            h: 40.0,
        },
    }]
}

fn references() -> Vec<Reference> {
    vec![
        Reference {
            file_ref: "/OFD.xml".to_owned(),
            check_value_base64: "abc=".to_owned(),
        },
        Reference {
            file_ref: "/Doc_0/Document.xml".to_owned(),
            check_value_base64: "def=".to_owned(),
        },
    ]
}

#[test]
fn signature_xml_has_fixture_shape() {
    let stamps = stamps();
    let references = references();
    let xml = signature_xml(&SignatureXmlInput {
        signed_value_loc: "/Doc_0/Signs/Sign_0/SignedValue.dat",
        provider_name: "ofdrw-sign",
        provider_company: Some("ofdrw"),
        provider_version: Some("1.2.0"),
        date_time: "2026-11-24T10:00:00",
        seal_base_loc: Some("/Doc_0/Signs/Sign_0/Seal.esl"),
        stamps: &stamps,
        references: &references,
    });
    assert!(xml.starts_with(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<ofd:Signature xmlns:ofd=\"http://www.ofdspec.org/2016\">"
    ));
    let ordered = [
        "<ofd:SignedValue>/Doc_0/Signs/Sign_0/SignedValue.dat</ofd:SignedValue>",
        "<ofd:SignedInfo>",
        "<ofd:Provider ProviderName=\"ofdrw-sign\" Company=\"ofdrw\" Version=\"1.2.0\"/>",
        "<ofd:SignatureMethod>1.2.156.10197.1.501</ofd:SignatureMethod>",
        "<ofd:SignatureDateTime>2026-11-24 10:00:00</ofd:SignatureDateTime>",
        "<ofd:Seal><ofd:BaseLoc>/Doc_0/Signs/Sign_0/Seal.esl</ofd:BaseLoc></ofd:Seal>",
        "<ofd:StampAnnot ID=\"2\" Boundary=\"50 50 40 40\" PageRef=\"1\"/>",
        "<ofd:References CheckMethod=\"SM3\">",
        "<ofd:Reference FileRef=\"/OFD.xml\"><ofd:CheckValue>abc=</ofd:CheckValue></ofd:Reference>",
        "<ofd:Reference FileRef=\"/Doc_0/Document.xml\"><ofd:CheckValue>def=</ofd:CheckValue></ofd:Reference>",
        "</ofd:References></ofd:SignedInfo></ofd:Signature>",
    ];
    let positions: Vec<usize> = ordered
        .iter()
        .map(|part| xml.find(part).unwrap_or_else(|| panic!("{part} in\n{xml}")))
        .collect();
    assert!(
        positions.windows(2).all(|pair| pair[0] < pair[1]),
        "elements in GB/T 33190 order"
    );
    assert!(
        xml.ends_with(ordered[ordered.len() - 1]),
        "no trailing newline: these bytes are hashed"
    );
}

#[test]
fn signature_xml_omits_what_is_absent_and_escapes() {
    let xml = signature_xml(&SignatureXmlInput {
        signed_value_loc: "Sign_0/SignedValue.dat",
        provider_name: "a&b <c>",
        provider_company: None,
        provider_version: None,
        date_time: NOW,
        seal_base_loc: None,
        stamps: &[],
        references: &[Reference {
            file_ref: "/x&y/<z>.xml".to_owned(),
            check_value_base64: "AA==".to_owned(),
        }],
    });
    assert!(!xml.contains("<ofd:Seal>"), "no seal, no Seal element");
    assert!(!xml.contains("<ofd:StampAnnot"), "no stamps, no StampAnnot");
    assert!(xml.contains("ProviderName=\"a&amp;b &lt;c&gt;\" Company=\"\" Version=\"\""));
    assert!(xml.contains("<ofd:SignedValue>Sign_0/SignedValue.dat</ofd:SignedValue>"));
    assert!(xml.contains("FileRef=\"/x&amp;y/&lt;z&gt;.xml\""));
}

#[test]
fn generalized_time_converts_the_wall_clock_form() {
    assert_eq!(
        generalized_time("2026-11-24 10:00:00").unwrap(),
        "20261124100000Z"
    );
    assert_eq!(
        generalized_time("2026-11-24T10:00:00").unwrap(),
        "20261124100000Z"
    );
    assert!(generalized_time("yesterday").is_err());
    assert!(generalized_time("2026-13-40 99:00:00").is_err());
}

// ---- SES v4 ----

fn seal_from(ca: &TestCa) -> (Vec<u8>, rofd_sign::TestSigner) {
    let mut signer = ca.signer();
    let png = tiny_png();
    let e_seal_info = build_e_seal_info(
        &SealInput {
            png: &png,
            width: 40.0,
            height: 39.6,
            certificate_der: signer.certificate_der(),
        },
        NOW,
    )
    .expect("seal info");
    let maker_signature = signer.sign(&e_seal_info).expect("maker signature");
    let seal = finish_seal(&e_seal_info, &maker_signature, signer.certificate_der()).expect("seal");
    (seal, signer)
}

#[test]
fn ses_v4_encoder_output_parses_back() {
    let ca = ca();
    let (seal, mut signer) = seal_from(&ca);
    let certificate = parse_certificate(signer.certificate_der()).unwrap();

    // The seal carries its picture where readers expect it.
    let picture = rofd_io::seal_picture_from_der(&seal).expect("picture in the seal");
    assert_eq!(
        (picture.width, picture.height),
        (40.0, 40.0),
        "integers, rounded"
    );
    assert_eq!(picture.png.as_slice(), tiny_png().as_slice());

    // Seal.esl ::= SEQUENCE { eSealInfo, certificate, algorithm, signature }.
    let root = parse_one(&seal).unwrap();
    let seal_kids = parse_children(&root).unwrap();
    assert_eq!(
        child_tags(&root),
        vec![TAG_SEQUENCE, TAG_OCTET_STRING, 0x06, 0x03]
    );
    assert_eq!(seal_kids[1].content, signer.certificate_der());
    assert_eq!(seal_kids[2].as_oid().unwrap(), "1.2.156.10197.1.501");
    // eSealInfo ::= SEQUENCE { header, id, property, picture }.
    let info = parse_children(&seal_kids[0]).unwrap();
    assert_eq!(
        child_tags(&seal_kids[0]),
        vec![TAG_SEQUENCE, TAG_IA5_STRING, TAG_SEQUENCE, TAG_SEQUENCE]
    );
    let header = parse_children(&info[0]).unwrap();
    assert_eq!(
        child_tags(&info[0]),
        vec![TAG_IA5_STRING, TAG_INTEGER, TAG_IA5_STRING]
    );
    assert_eq!(header[0].as_text().unwrap(), "ES");
    assert_eq!(header[1].as_i64().unwrap(), 4, "the GB/T 38540 seal head");
    assert_eq!(info[1].as_text().unwrap().len(), 32, "a 32-hex seal id");
    assert_eq!(
        child_tags(&info[2]),
        vec![
            TAG_INTEGER,
            0x0C,
            TAG_INTEGER,
            TAG_SEQUENCE,
            0x18,
            0x18,
            0x18
        ]
    );
    // The certificate list is a list: each certificate in an OCTET STRING
    // (SES_CertList / CertInfoList), not the certificate's own SEQUENCE.
    let certificate_list = parse_children(&info[2]).unwrap()[3];
    assert_eq!(child_tags(&certificate_list), vec![TAG_OCTET_STRING]);
    assert_eq!(
        parse_children(&certificate_list).unwrap()[0].content,
        signer.certificate_der(),
        "the seal certificate is embedded verbatim"
    );
    assert_ne!(
        parse_children(&certificate_list).unwrap()[0].tag,
        TAG_SEQUENCE,
        "a bare certificate would read as a list whose first item is the TBSCertificate"
    );

    // TBS and the signature value read back with the production reader.
    let description = b"<ofd:Signature/>";
    let data_hash = sm3(description);
    let tbs = build_tbs(&seal, &data_hash, "/Doc_0/Signs/Sign_0/Signature.xml", NOW).unwrap();
    let signature = signer.sign(&tbs).expect("sign the TBS");
    let signed_value = finish_signature(&tbs, &signature, signer.certificate_der()).unwrap();
    let ses = parse_ses_signature(&signed_value).unwrap();
    assert_eq!(ses.version, 4);
    assert_eq!(ses.tbs_raw, tbs.as_slice(), "the TBS is embedded verbatim");
    assert_eq!(ses.cert_der, signer.certificate_der());
    assert_eq!(ses.data_hash, data_hash);
    assert_eq!(ses.signature_der, signature.as_slice());
    assert_eq!(
        ses.embedded_seal_raw,
        seal.as_slice(),
        "the embedded seal equals the standalone Seal.esl"
    );
    assert_eq!(ses.time.as_deref(), Some("20261124100000Z"));
    assert!(sm2_verify(
        &certificate.public_key_sec1,
        DEFAULT_SM2_USER_ID,
        ses.tbs_raw,
        ses.signature_der
    ));
}

#[test]
fn ses_encoder_rejects_unusable_input() {
    let ca = ca();
    let der = ca.cert_der.clone();
    let png = tiny_png();
    let seal_info = |png: &[u8], width: f64, height: f64| {
        build_e_seal_info(
            &SealInput {
                png,
                width,
                height,
                certificate_der: &der,
            },
            NOW,
        )
    };
    assert!(seal_info(&[], 40.0, 40.0).is_err(), "no picture");
    assert!(seal_info(b"not a png", 40.0, 40.0).is_err());
    assert!(seal_info(&png, 0.0, 40.0).is_err(), "a zero-size seal");
    assert!(seal_info(&png, f64::NAN, 40.0).is_err());
    assert!(seal_info(&png, -1.0, 40.0).is_err());
    assert!(seal_info(&png, 40.0, 40.0).is_ok());
    let mut bad = ca.clone();
    bad.cert_der = b"not a certificate".to_vec();
    assert!(
        build_e_seal_info(
            &SealInput {
                png: &png,
                width: 40.0,
                height: 40.0,
                certificate_der: &bad.cert_der,
            },
            NOW
        )
        .is_err(),
        "the seal certificate must be a certificate"
    );
    assert!(build_e_seal_info(
        &SealInput {
            png: &png,
            width: 40.0,
            height: 40.0,
            certificate_der: &der
        },
        "not a time"
    )
    .is_err());
}

// ---- GB/T 35275 ----

#[test]
fn gbt35275_round_trips() {
    let ca = ca();
    let mut signer = ca.signer();
    let plaintext = sm3(b"the signature description file");
    let signature = signer.sign(&plaintext).unwrap();
    let message = build_signed_data(&plaintext, &signature, signer.certificate_der()).unwrap();

    let parsed = parse_signed_data(&message).unwrap();
    assert_eq!(parsed.plaintext, plaintext);
    assert_eq!(parsed.signature, signature.as_slice());
    assert_eq!(parsed.cert_der, signer.certificate_der());

    // ContentInfo ::= SEQUENCE { signedData OID, [0] SignedData }.
    let root = parse_one(&message).unwrap();
    let kids = parse_children(&root).unwrap();
    assert_eq!(child_tags(&root), vec![0x06, 0xA0]);
    assert_eq!(kids[0].as_oid().unwrap(), "1.2.156.10197.6.1.4.2.2");
    let signed_data = parse_one(kids[1].content).unwrap();
    assert_eq!(
        child_tags(&signed_data),
        vec![TAG_INTEGER, 0x31, TAG_SEQUENCE, 0xA0, 0x31],
        "version, digestAlgorithms, contentInfo, [0] certificates, signerInfos"
    );
    let sd = parse_children(&signed_data).unwrap();
    assert_eq!(sd[0].as_i64().unwrap(), 1);
    assert_eq!(
        parse_one(sd[3].content).unwrap().raw,
        signer.certificate_der()
    );
    let content_info = parse_children(&sd[2]).unwrap();
    assert_eq!(child_tags(&sd[2]), vec![0x06, 0xA0]);
    assert_eq!(content_info[0].as_oid().unwrap(), "1.2.156.10197.6.1.4.2.1");
    assert_eq!(child_tags(&content_info[1]), vec![TAG_OCTET_STRING]);
    let signer_info = parse_children(&sd[4]).unwrap()[0];
    assert_eq!(
        child_tags(&signer_info),
        vec![
            TAG_INTEGER,
            TAG_SEQUENCE,
            TAG_SEQUENCE,
            TAG_SEQUENCE,
            TAG_OCTET_STRING
        ],
        "no authenticated attributes"
    );
    let si = parse_children(&signer_info).unwrap();
    assert_eq!(
        parse_children(&si[2]).unwrap()[0].as_oid().unwrap(),
        "1.2.156.10197.1.401",
        "digestAlgorithm is SM3"
    );
    assert_eq!(
        parse_children(&si[3]).unwrap()[0].as_oid().unwrap(),
        "1.2.156.10197.1.301.1",
        "digestEncryptionAlgorithm is the SM2 signature algorithm"
    );
    assert_eq!(si[4].content, signature.as_slice());

    assert!(parse_signed_data(b"garbage").is_err());
    assert!(parse_signed_data(&[]).is_err());
    for end in 0..message.len() {
        let _ = parse_signed_data(&message[..end]);
    }
}

// ---- verification of a GB/T 35275 value ----

fn fixture(name: &str) -> Vec<u8> {
    std::fs::read(format!("tests/fixtures/{name}")).unwrap()
}

fn entry(bytes: &[u8], name: &str) -> Vec<u8> {
    read_all_entries(bytes)
        .unwrap()
        .into_iter()
        .find(|(n, _)| n == name)
        .unwrap()
        .1
}

fn with_value(package: &[u8], value: &[u8]) -> Vec<u8> {
    const NAME: &str = "Doc_0/Signs/Sign_0/SignedValue.dat";
    let mut entries: Vec<(String, Vec<u8>)> = read_all_entries(package)
        .unwrap()
        .into_iter()
        .filter(|(n, _)| n != NAME)
        .collect();
    entries.push((NAME.to_owned(), value.to_vec()));
    write_zip(&entries).unwrap()
}

#[test]
fn verify_handles_gbt35275_signature_value() {
    let package = fixture("DigitalSign.ofd");
    let description = entry(&package, "Doc_0/Signs/Sign_0/Signature.xml");
    let ca = ca();
    let mut signer = ca.signer();
    let signature = signer.sign(&sm3(&description)).unwrap();
    let message =
        build_signed_data(&sm3(&description), &signature, signer.certificate_der()).unwrap();

    let report = verify(&with_value(&package, &message), &VerifyOptions::default());
    assert_eq!(report.len(), 1);
    let report = &report[0];
    assert_eq!(report.variant, rofd_sign::SignatureVariant::Gbt35275);
    assert_eq!(
        report.signature_value.status,
        rofd_sign::SignatureValueStatus::Ok,
        "{report:?}"
    );
    assert_eq!(report.overall, SignatureOverall::Valid);
    assert_eq!(
        report.certificate.as_ref().unwrap().subject,
        "rofd test signer"
    );

    // The digest inside no longer matches the description file.
    let other = entry(&package, "Doc_0/Pages/Page_0/Content.xml");
    let signature = signer.sign(&sm3(&other)).unwrap();
    let message = build_signed_data(&sm3(&other), &signature, signer.certificate_der()).unwrap();
    let report = &verify(&with_value(&package, &message), &VerifyOptions::default())[0];
    assert_eq!(
        report.signature_value.status,
        rofd_sign::SignatureValueStatus::Fail
    );
    assert_eq!(report.overall, SignatureOverall::Invalid);

    // A message by another key: the embedded certificate does not verify it.
    let other_ca = TestCa::from_seed("someone else", b"other-seed");
    let forged = build_signed_data(&sm3(&description), &signature, &other_ca.cert_der).unwrap();
    let report = &verify(&with_value(&package, &forged), &VerifyOptions::default())[0];
    assert_eq!(
        report.signature_value.status,
        rofd_sign::SignatureValueStatus::Fail
    );
    assert_eq!(report.overall, SignatureOverall::Invalid);
}

// ---- the seal's shape against the ofdrw sample ----

/// The tag tree of a node with every value left out, e.g.
/// `30(02,0c,02,30(04),18,18,18)`: the certificate list is a SEQUENCE that
/// holds an OCTET STRING.
fn shape(node: &rofd_sign::DerNode<'_>) -> String {
    let tag = format!("{:02x}", node.tag);
    if node.tag & 0x20 == 0 {
        return tag;
    }
    let children: Vec<String> = parse_children(node).unwrap().iter().map(shape).collect();
    format!("{tag}({})", children.join(","))
}

#[test]
fn seal_has_the_shape_of_the_ofdrw_sample() {
    let (seal, _) = seal_from(&ca());
    let sample = entry(&fixture("SESV4SignDoc.ofd"), "Doc_0/Signs/Sign_0/Seal.esl");
    fn root(der: &[u8]) -> rofd_sign::DerNode<'_> {
        parse_one(der).unwrap()
    }
    fn info(der: &[u8]) -> rofd_sign::DerNode<'_> {
        parse_children(&root(der)).unwrap()[0]
    }
    assert_eq!(
        shape(&info(&seal)),
        shape(&info(&sample)),
        "eSealInfo: header, id, property with its certificate list, picture"
    );
    assert_eq!(
        shape(&root(&seal)),
        shape(&root(&sample)),
        "ESS_Seal: eSealInfo, certificate, algorithm, maker signature"
    );
}
