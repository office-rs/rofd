//! The signer's side of the two-step flow: refusals, a picture-less
//! signature, a certificate file, the provider details.

use super::*;

#[test]
fn no_picture_signature_is_valid_without_stamp_annot() {
    let signed = sign(
        &bare_package(),
        &mut first_ca().signer(),
        &SignOptions::new(NOW),
    )
    .expect("sign");
    let reports = reports_of(&signed);
    assert_eq!(reports.len(), 1);
    let report = &reports[0];
    assert_eq!(report.overall, SignatureOverall::Valid);
    assert_eq!(report.kind, SignatureKind::Sign);
    assert_eq!(report.variant, SignatureVariant::Gbt35275);
    assert_eq!(report.seal_match, None);
    assert_eq!(report.signature_value.status, SignatureValueStatus::Ok);
    assert_eq!(
        report.certificate.as_ref().map(|c| c.subject.as_str()),
        Some("rofd test signer")
    );

    let all = entries(&signed);
    assert!(option(&all, "Doc_0/Signs/Sign_0/Seal.esl").is_none());
    let description = text(&all, "Doc_0/Signs/Sign_0/Signature.xml");
    assert!(!description.contains("<ofd:Seal>"));
    assert!(!description.contains("StampAnnot"));
    let list = text(&all, "Doc_0/Signs/Signatures.xml");
    assert!(list.contains(
        r#"<ofd:Signature ID="1" Type="Sign" BaseLoc="/Doc_0/Signs/Sign_0/Signature.xml"/>"#
    ));
    assert!(list.contains("<ofd:MaxSignId>1</ofd:MaxSignId>"));
    assert!(stamps(&signed).is_empty());

    // What is signed is the digest of the description file.
    let signed_data =
        rofd_sign::parse_signed_data(entry(&all, "Doc_0/Signs/Sign_0/SignedValue.dat"))
            .expect("a GB/T 35275 message");
    assert_eq!(
        signed_data.plaintext,
        rofd_sign::sm3(description.as_bytes())
    );
}

#[test]
fn failing_sign_returns_err() {
    let document = bare_package();
    let mut signer = first_ca().signer();

    assert!(
        is_invalid(sign(
            &document,
            &mut signer,
            &options(Placement::At {
                page: 9,
                boundary: BOTTOM_RIGHT
            })
        )),
        "page out of range"
    );
    assert!(
        is_invalid(sign(
            &document,
            &mut signer,
            &seal_of(b"not an image".to_vec(), 0.0)
        )),
        "the picture cannot be decoded"
    );
    assert!(
        is_invalid(sign(
            &document,
            &mut signer,
            &SignOptions {
                now: "tomorrow".into(),
                ..seal_options()
            }
        )),
        "not a time"
    );

    // An unsigned Document.xml without MaxUnitID has no place for the ID
    // reserve.
    let files: Vec<(String, Vec<u8>)> = vec![
        ("OFD.xml".to_owned(), ofd_xml().into_bytes()),
        (
            "Doc_0/Document.xml".to_owned(),
            document_xml(&two_pages(), None, false).into_bytes(),
        ),
        (
            "Doc_0/Pages/Page_0/Content.xml".to_owned(),
            page(two_pages()[0]).into_bytes(),
        ),
        (
            "Doc_0/Pages/Page_1/Content.xml".to_owned(),
            page(two_pages()[1]).into_bytes(),
        ),
    ];
    let anchorless = zip_of(&files);
    let Err(error) = sign(&anchorless, &mut signer, &seal_options()) else {
        panic!("expected an error")
    };
    assert!(matches!(error, SignError::Encode { .. }), "{error}");
    assert!(error.to_string().contains("MaxUnitID"), "{error}");

    struct Refusing {
        certificate: Vec<u8>,
    }
    impl Signer for Refusing {
        fn certificate_der(&self) -> &[u8] {
            &self.certificate
        }
        fn sign(&mut self, _: &[u8]) -> Result<Vec<u8>, SignError> {
            Err(SignError::Signer {
                reason: "the token refused".to_owned(),
            })
        }
    }
    let mut refusing = Refusing {
        certificate: first_ca().cert_der,
    };
    let result = sign(&document, &mut refusing, &seal_options());
    assert!(matches!(result, Err(SignError::Signer { .. })));

    // Not an OFD package.
    let result = sign(b"not a package", &mut signer, &seal_options());
    assert!(matches!(result, Err(SignError::Io(_))));
}

#[test]
fn a_signature_that_does_not_verify_is_rejected() {
    // A signer that signs with another key than its certificate says: the
    // result would not verify, so it is not written.
    struct Mismatched {
        certificate: Vec<u8>,
        key: TestCa,
    }
    impl Signer for Mismatched {
        fn certificate_der(&self) -> &[u8] {
            &self.certificate
        }
        fn sign(&mut self, to_sign: &[u8]) -> Result<Vec<u8>, SignError> {
            self.key.signer().sign(to_sign)
        }
    }
    for opts in [seal_options(), SignOptions::new(NOW)] {
        let mut mismatched = Mismatched {
            certificate: first_ca().cert_der,
            key: ca("someone else", b"seed-b"),
        };
        let result = sign(&bare_package(), &mut mismatched, &opts);
        let Err(error) = result else {
            panic!("expected an error")
        };
        assert!(matches!(error, SignError::Signer { .. }), "{error}");
    }
}

// ---- two-phase and real certificate files ----

#[test]
fn two_phase_matches_one_step() {
    for opts in [seal_options(), SignOptions::new(NOW)] {
        let package = bare_package();
        let ca = first_ca();
        let one = sign(&package, &mut ca.signer(), &opts).expect("sign");

        let mut signer = ca.signer();
        let mut rounds = 0;
        let mut step =
            SignStep::More(prepare_sign(&package, &ca.cert_der, &opts).expect("prepare"));
        let bytes = loop {
            step = match step {
                SignStep::More(prepared) => {
                    let signature = signer.sign(prepared.to_sign()).expect("sign");
                    rounds += 1;
                    prepared.apply(&signature).expect("apply")
                }
                SignStep::Done(bytes) => break bytes,
            };
        };
        assert_eq!(rounds, if opts.seal.is_some() { 2 } else { 1 });
        assert_eq!(bytes, one, "both paths write the same package");
    }
}

#[test]
fn applying_an_empty_signature_is_an_error() {
    let package = bare_package();
    let ca = first_ca();
    let opts = seal_options();
    let prepared = prepare_sign(&package, &ca.cert_der, &opts).expect("prepare");
    assert!(!prepared.to_sign().is_empty());
    assert!(matches!(prepared.apply(&[]), Err(SignError::Signer { .. })));

    // The second round of a seal is checked the same way.
    let prepared = prepare_sign(&package, &ca.cert_der, &opts).expect("prepare");
    let first = ca.signer().sign(prepared.to_sign()).expect("sign");
    let SignStep::More(prepared) = prepared.apply(&first).expect("first round") else {
        panic!("a seal takes a second round")
    };
    assert!(matches!(prepared.apply(&[]), Err(SignError::Signer { .. })));
    // A digital signature takes one round only.
    let prepared = prepare_sign(&package, &ca.cert_der, &SignOptions::new(NOW)).expect("prepare");
    assert!(matches!(prepared.apply(&[]), Err(SignError::Signer { .. })));
}

#[test]
fn pfx_signer_signs_end_to_end() {
    let user_p12 = std::fs::read("tests/fixtures/USER.p12").expect("fixture");
    let mut signer = PfxSigner::open(&user_p12, "777777").expect("open");
    let signed = sign(&annotated_package(), &mut signer, &seal_options()).expect("sign");
    let reports = reports_of(&signed);
    assert_eq!(reports.len(), 1);
    assert_eq!(reports[0].overall, SignatureOverall::Valid);
    assert_eq!(
        reports[0].certificate.as_ref().map(|c| c.subject.as_str()),
        Some("Test Certificate")
    );

    // A second signer, from a file with modern protection.
    let openssl_p12 = std::fs::read("tests/fixtures/sm2_test.p12").expect("fixture");
    let mut other = PfxSigner::open(&openssl_p12, "changeit").expect("open");
    let twice = sign(&signed, &mut other, &SignOptions::new(NOW)).expect("second");
    assert!(all_valid(&twice));
    assert_eq!(reports_of(&twice).len(), 2);
}

#[test]
fn provider_details_are_written() {
    let opts = SignOptions {
        provider: Provider {
            name: "Acme & Co".into(),
            company: Some("Acme <sign>".into()),
            version: Some("1.0".into()),
        },
        ..seal_options()
    };
    let signed = sign(&bare_package(), &mut first_ca().signer(), &opts).expect("sign");
    let description = text(&entries(&signed), "Doc_0/Signs/Sign_0/Signature.xml");
    assert!(
        description.contains(
            "ProviderName=\"Acme &amp; Co\" Company=\"Acme &lt;sign&gt;\" Version=\"1.0\""
        ),
        "{description}"
    );
    assert!(all_valid(&signed));
}

// ---- scaffolds and gates ----
