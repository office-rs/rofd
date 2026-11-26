//! The signing line over wasm (spec 2026-11-23 §12): verification, signing
//! with a certificate file, and the two-phase form for a signer outside this
//! process (spec §10.4).
//!
//! Options and reports cross the boundary as JSON. The JSON shapes are
//! parsed and produced by plain functions that need no wasm types, so they
//! are tested natively; the `#[wasm_bindgen]` exports (wasm32 only) are thin
//! over them.
//!
//! - `options_json` of signing is [`rofd_sign::SignOptions`] in camelCase:
//!   `seal` is `null` or `{ png: number[] | string (base64), sizeMm? }`;
//!   `placement` is `null`, `{ at: { page, boundary: { x, y, w, h } } }` or
//!   `{ bottomRight: { pages: "last" | "all", rightMm, bottomMm } }`; `now`
//!   is `YYYY-MM-DD HH:mm:ss`; `provider` may be left out. A field that is
//!   not known is an error, not ignored.
//! - `options_json` of verification is `{ now?, certs?: { [id]: base64 } }`.
//! - The reports keep rofd-sign's snake_case field names.

use base64::Engine as _;
use rofd_dom::Rect;
use rofd_sign::{
    Pages, Placement, Provider, SealImage, SignError, SignOptions, SignatureReport, VerifyOptions,
};
use serde::Deserialize;
use std::collections::HashMap;

// ---- the JSON shapes ----

/// The JSON form of [`SignOptions`].
#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase", deny_unknown_fields)]
struct OptionsJson {
    seal: Option<SealJson>,
    placement: Option<PlacementJson>,
    now: String,
    provider: Option<ProviderJson>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SealJson {
    png: PngJson,
    /// Left out: `0.0`, which means the default size.
    #[serde(default)]
    size_mm: f64,
}

/// The picture as bytes, or as base64.
#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum PngJson {
    Bytes(Vec<u8>),
    Base64(String),
}

#[derive(Debug, Deserialize)]
#[serde(
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
enum PlacementJson {
    At {
        page: u32,
        boundary: Rect,
    },
    BottomRight {
        pages: PagesJson,
        right_mm: f64,
        bottom_mm: f64,
    },
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "lowercase")]
enum PagesJson {
    Last,
    All,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase", deny_unknown_fields)]
struct ProviderJson {
    name: String,
    company: Option<String>,
    version: Option<String>,
}

/// The JSON form of [`VerifyOptions`].
#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase", deny_unknown_fields)]
struct VerifyOptionsJson {
    now: Option<String>,
    /// Certificates the package does not carry, by signature ID, as base64.
    certs: HashMap<String, String>,
}

fn bad_options(what: &str, error: impl std::fmt::Display) -> SignError {
    SignError::InvalidInput {
        reason: format!("the {what} are not valid JSON options: {error}"),
    }
}

fn bad_base64(what: &str, error: impl std::fmt::Display) -> SignError {
    SignError::InvalidInput {
        reason: format!("{what} is not base64: {error}"),
    }
}

// ---- parsing and producing, without wasm types ----

/// Parse the JSON form of the signing options.
pub fn parse_sign_options(options_json: &str) -> Result<SignOptions, SignError> {
    let json: OptionsJson =
        serde_json::from_str(options_json).map_err(|e| bad_options("signing options", e))?;
    let seal = json
        .seal
        .map(|seal| {
            let png = match seal.png {
                PngJson::Bytes(bytes) => bytes,
                PngJson::Base64(text) => base64::engine::general_purpose::STANDARD
                    .decode(text)
                    .map_err(|e| bad_base64("the seal picture", e))?,
            };
            Ok::<_, SignError>(SealImage {
                png,
                size_mm: seal.size_mm,
            })
        })
        .transpose()?;
    let placement = json.placement.map(|placement| match placement {
        PlacementJson::At { page, boundary } => Placement::At { page, boundary },
        PlacementJson::BottomRight {
            pages,
            right_mm,
            bottom_mm,
        } => Placement::BottomRight {
            pages: match pages {
                PagesJson::Last => Pages::Last,
                PagesJson::All => Pages::All,
            },
            right_mm,
            bottom_mm,
        },
    });
    Ok(SignOptions {
        seal,
        placement,
        now: json.now,
        // Left out: rofd-sign's default provider.
        provider: json
            .provider
            .map_or_else(Provider::default, |provider| Provider {
                name: provider.name,
                company: provider.company,
                version: provider.version,
            }),
    })
}

/// Parse the JSON form of the verification options.
pub fn parse_verify_options(options_json: &str) -> Result<VerifyOptions, SignError> {
    let json: VerifyOptionsJson =
        serde_json::from_str(options_json).map_err(|e| bad_options("verification options", e))?;
    let certs = json
        .certs
        .into_iter()
        .map(|(id, certificate)| {
            let der = base64::engine::general_purpose::STANDARD
                .decode(certificate)
                .map_err(|e| bad_base64("a certificate", e))?;
            Ok((id, der))
        })
        .collect::<Result<_, SignError>>()?;
    Ok(VerifyOptions {
        now: json.now,
        certs,
    })
}

/// The reports as JSON.
pub fn reports_json(reports: &[SignatureReport]) -> Result<String, SignError> {
    serde_json::to_string(reports).map_err(|error| SignError::Encode {
        reason: format!("the verification reports cannot be written as JSON: {error}"),
    })
}

/// Verify a package and return the reports as JSON. A package that cannot
/// be opened, or options that are not valid JSON, yield `[]`: verification
/// reports, it does not fail.
pub fn verify_json(ofd: &[u8], options_json: &str) -> String {
    let reports = match parse_verify_options(options_json) {
        Ok(options) => rofd_sign::verify(ofd, &options),
        Err(_) => Vec::new(),
    };
    reports_json(&reports).unwrap_or_else(|_| "[]".to_owned())
}

/// Sign a package with a certificate file (`.pfx`, `.p12`) and its password.
pub fn sign_with_pfx(
    ofd: &[u8],
    pfx: &[u8],
    password: &str,
    options_json: &str,
) -> Result<Vec<u8>, SignError> {
    let options = parse_sign_options(options_json)?;
    let mut signer = rofd_sign::PfxSigner::open(pfx, password)?;
    rofd_sign::sign(ofd, &mut signer, &options)
}

/// What to show for a certificate file, as JSON: the entry the certificate
/// store would hold ([`rofd_sign::CertificateEntry`]).
pub fn certificate_info_json(pfx: &[u8], password: &str) -> Result<String, SignError> {
    let mut store = rofd_sign::CertificateStore::default();
    let entry = store.import(pfx.to_vec(), password)?;
    serde_json::to_string(&entry).map_err(|error| SignError::Encode {
        reason: format!("the certificate cannot be written as JSON: {error}"),
    })
}

// ---- the two-phase form on the open document ----

#[cfg(any(target_arch = "wasm32", test))]
use rofd_component::{
    PreparedSignature as ComponentPreparedSignature, SignStep as ComponentSignStep,
};

/// What one round of the two-phase form on the open document comes to, as
/// plain data: the next round to sign, or - the signature being in the
/// document - the reports as JSON. It exists where its readers are: the
/// wasm exports, and the test.
#[cfg(any(target_arch = "wasm32", test))]
pub(crate) enum StepOutcome {
    /// Read where the wasm exports are compiled, and nowhere else.
    #[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
    Next(ComponentPreparedSignature),
    Landed(String),
}

/// [`StepOutcome`] of what the component's `finish_sign` returned. The wasm
/// types wrap it; this part is plain, and tested natively.
#[cfg(any(target_arch = "wasm32", test))]
pub(crate) fn step_result(step: ComponentSignStep) -> Result<StepOutcome, SignError> {
    Ok(match step {
        ComponentSignStep::More(next) => StepOutcome::Next(next),
        ComponentSignStep::Done(reports) => StepOutcome::Landed(reports_json(&reports)?),
    })
}

// ---- the wasm exports ----

#[cfg(target_arch = "wasm32")]
mod wasm_exports {
    use super::*;
    use wasm_bindgen::prelude::*;

    /// Verify every signature of a package. The reports as a JSON array; `[]`
    /// when the package cannot be opened or declares no signature.
    #[wasm_bindgen(js_name = verifySignatures)]
    pub fn verify_signatures(ofd: &[u8], options_json: &str) -> String {
        verify_json(ofd, options_json)
    }

    /// Sign a package with a certificate file and its password. Returns the
    /// signed package; the input is not changed.
    #[wasm_bindgen(js_name = signOfd)]
    pub fn sign_ofd(
        ofd: &[u8],
        pfx: &[u8],
        password: &str,
        options_json: &str,
    ) -> Result<Vec<u8>, JsValue> {
        sign_with_pfx(ofd, pfx, password, options_json).map_err(js_error)
    }

    /// Read a certificate file for display: the certificate store's entry
    /// for it, as JSON.
    #[wasm_bindgen(js_name = certificateInfo)]
    pub fn certificate_info(pfx: &[u8], password: &str) -> Result<String, JsValue> {
        certificate_info_json(pfx, password).map_err(js_error)
    }

    /// Start signing for a signer outside this process: everything is
    /// prepared up to the first signature it has to produce.
    /// `certificate_der` is the signer's certificate.
    #[wasm_bindgen(js_name = signPrepare)]
    pub fn sign_prepare(
        ofd: &[u8],
        certificate_der: &[u8],
        options_json: &str,
    ) -> Result<PreparedSign, JsValue> {
        let options = parse_sign_options(options_json).map_err(js_error)?;
        rofd_sign::prepare_sign(ofd, certificate_der, &options)
            .map(|prepared| PreparedSign {
                inner: Some(prepared),
            })
            .map_err(js_error)
    }

    /// A package with everything a signature needs but the signature itself.
    #[wasm_bindgen]
    pub struct PreparedSign {
        /// `None` once the round was applied.
        inner: Option<rofd_sign::Prepared>,
    }

    #[wasm_bindgen]
    impl PreparedSign {
        /// What the signer signs in this round.
        #[wasm_bindgen(js_name = toSign)]
        pub fn to_sign(&self) -> Vec<u8> {
            self.inner
                .as_ref()
                .map_or_else(Vec::new, |prepared| prepared.to_sign().to_vec())
        }

        /// Take the signature of `toSign` and go on.
        #[wasm_bindgen(js_name = apply)]
        pub fn apply(self, signature: &[u8]) -> Result<StepResult, JsValue> {
            let prepared = self
                .inner
                .ok_or_else(|| JsValue::from_str("this signing step was already applied"))?;
            match prepared.apply(signature).map_err(js_error)? {
                rofd_sign::SignStep::More(next) => Ok(StepResult {
                    bytes: None,
                    next: Some(PreparedSign { inner: Some(next) }),
                }),
                rofd_sign::SignStep::Done(bytes) => Ok(StepResult {
                    bytes: Some(bytes),
                    next: None,
                }),
            }
        }
    }

    /// What [`PreparedSign::apply`] returns: the signed package, or the
    /// next round to sign.
    #[wasm_bindgen]
    pub struct StepResult {
        bytes: Option<Vec<u8>>,
        next: Option<PreparedSign>,
    }

    #[wasm_bindgen]
    impl StepResult {
        /// Whether the package is signed.
        #[wasm_bindgen(getter, js_name = done)]
        pub fn done(&self) -> bool {
            self.bytes.is_some()
        }

        /// The signed package; empty unless `done`.
        #[wasm_bindgen(getter, js_name = bytes)]
        pub fn bytes(&self) -> Vec<u8> {
            self.bytes.clone().unwrap_or_default()
        }

        /// The next round to sign; `undefined` when the package is signed.
        /// Consumes the step result.
        #[wasm_bindgen(js_name = next)]
        pub fn next(self) -> Option<PreparedSign> {
            self.next
        }
    }

    fn js_error(error: SignError) -> JsValue {
        JsValue::from_str(&error.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rofd_sign::{
        CertificateStore, Pages, Placement, Provider, SealImage, SignOptions, SignatureOverall,
        VerifyOptions,
    };

    const NOW: &str = "2026-11-24 10:30:00";
    const SIGN_FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../sign/tests/fixtures/");

    fn fixture(name: &str) -> Vec<u8> {
        std::fs::read(format!("{SIGN_FIXTURES}/{name}"))
            .unwrap_or_else(|e| panic!("fixture {name}: {e}"))
    }

    /// A small opaque PNG.
    fn png() -> Vec<u8> {
        let mut bytes = std::io::Cursor::new(Vec::new());
        image::RgbaImage::from_pixel(8, 8, image::Rgba([190, 30, 30, 255]))
            .write_to(&mut bytes, image::ImageFormat::Png)
            .expect("PNG");
        bytes.into_inner()
    }

    /// An unsigned package: one A4 page, an attachment, nothing else.
    fn unsigned_package() -> Vec<u8> {
        let ofd_xml = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
<ofd:OFD xmlns:ofd=\"http://www.ofdspec.org/2016\"><ofd:DocBody>\
<ofd:DocRoot>Doc_0/Document.xml</ofd:DocRoot></ofd:DocBody></ofd:OFD>";
        let document = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
<ofd:Document xmlns:ofd=\"http://www.ofdspec.org/2016\">\
<ofd:CommonData><ofd:MaxUnitID>9</ofd:MaxUnitID>\
<ofd:PageArea><ofd:PhysicalBox>0 0 210 297</ofd:PhysicalBox></ofd:PageArea></ofd:CommonData>\
<ofd:Pages><ofd:Page ID=\"1\" BaseLoc=\"Pages/Page_0/Content.xml\"/></ofd:Pages>\
</ofd:Document>";
        let page = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
<ofd:Page xmlns:ofd=\"http://www.ofdspec.org/2016\">\
<ofd:Area><ofd:PhysicalBox>0 0 210 297</ofd:PhysicalBox></ofd:Area><ofd:Content/></ofd:Page>";
        rofd_io::zip_util::write_zip(&[
            ("OFD.xml".to_owned(), ofd_xml.as_bytes().to_vec()),
            (
                "Doc_0/Document.xml".to_owned(),
                document.as_bytes().to_vec(),
            ),
            (
                "Doc_0/Pages/Page_0/Content.xml".to_owned(),
                page.as_bytes().to_vec(),
            ),
            ("Doc_0/Attachs/note.txt".to_owned(), b"kept".to_vec()),
        ])
        .expect("zip")
    }

    fn seal_options_json() -> String {
        format!(
            "{{\"seal\":{{\"png\":{:?},\"sizeMm\":40.0}},\"now\":\"{NOW}\"}}",
            png()
        )
    }

    fn overalls(json: &str) -> Vec<String> {
        serde_json::from_str::<Vec<serde_json::Value>>(json)
            .expect("a JSON array")
            .into_iter()
            .map(|report| report["overall"].as_str().unwrap_or_default().to_owned())
            .collect()
    }

    #[test]
    fn options_json_parses_each_placement_form() {
        // A digital signature: no seal, no placement, the default provider.
        let options = parse_sign_options(&format!("{{\"now\":\"{NOW}\"}}")).expect("parse");
        assert_eq!(options, SignOptions::new(NOW));

        // A seal from picture bytes.
        let options = parse_sign_options(&seal_options_json()).expect("parse");
        assert_eq!(
            options
                .seal
                .as_ref()
                .map(|seal| (seal.png.clone(), seal.size_mm)),
            Some((png(), 40.0))
        );
        assert_eq!(
            options.placement, None,
            "the SDK's default position applies"
        );

        // A seal from base64, and the size left to the default.
        let encoded = base64::engine::general_purpose::STANDARD.encode(png());
        let options = parse_sign_options(&format!(
            "{{\"now\":\"{NOW}\",\"seal\":{{\"png\":\"{encoded}\"}},\"placement\":{{\
\"at\":{{\"page\":2,\"boundary\":{{\"x\":10.5,\"y\":20.0,\"w\":40.0,\"h\":30.0}}}}}}}}"
        ))
        .expect("parse");
        assert_eq!(options.seal.as_ref().map(|seal| seal.size_mm), Some(0.0));
        assert_eq!(
            options.placement,
            Some(Placement::At {
                page: 2,
                boundary: rofd_dom::Rect {
                    x: 10.5,
                    y: 20.0,
                    w: 40.0,
                    h: 30.0
                },
            })
        );

        for (pages, pages_json) in [(Pages::Last, "last"), (Pages::All, "all")] {
            let options = parse_sign_options(&format!(
                "{{\"now\":\"{NOW}\",\"seal\":null,\"placement\":{{\"bottomRight\":{{\
\"pages\":\"{pages_json}\",\"rightMm\":15.5,\"bottomMm\":8.0}}}}}}"
            ))
            .expect("parse");
            assert_eq!(
                options.placement,
                Some(Placement::BottomRight {
                    pages,
                    right_mm: 15.5,
                    bottom_mm: 8.0
                })
            );
        }

        // The provider, whole and in part.
        let options = parse_sign_options(&format!(
            "{{\"now\":\"{NOW}\",\"provider\":{{\"name\":\"Acme\",\"company\":\"Acme & Co\",\"version\":\"1.0\"}}}}"
        ))
        .expect("parse");
        assert_eq!(
            options.provider,
            Provider {
                name: "Acme".into(),
                company: Some("Acme & Co".into()),
                version: Some("1.0".into())
            }
        );
        let options = parse_sign_options(&format!(
            "{{\"now\":\"{NOW}\",\"provider\":{{\"name\":\"Acme\"}}}}"
        ))
        .expect("parse");
        assert_eq!(
            options.provider,
            Provider {
                name: "Acme".into(),
                company: None,
                version: None
            }
        );
    }

    #[test]
    fn options_json_rejects_unknown_pages_value() {
        let is_error = |json: &str| parse_sign_options(json).is_err();
        assert!(is_error(&format!(
            "{{\"now\":\"{NOW}\",\"placement\":{{\"bottomRight\":{{\"pages\":\"first\",\"rightMm\":10.0,\"bottomMm\":10.0}}}}}}"
        )), "an unknown pages value");
        assert!(
            is_error(&format!(
                "{{\"now\":\"{NOW}\",\"placement\":{{\"topLeft\":{{\"pages\":\"last\"}}}}}}"
            )),
            "an unknown placement form"
        );
        assert!(
            is_error(&format!(
                "{{\"now\":\"{NOW}\",\"seal\":{{\"png\":[1,2,3],\"size_mm\":40.0}}}}"
            )),
            "a field that is not camelCase is not ignored"
        );
        assert!(
            is_error(&format!(
                "{{\"now\":\"{NOW}\",\"seal\":{{\"png\":\"not base64!\",\"sizeMm\":40.0}}}}"
            )),
            "base64 that does not decode"
        );
        assert!(is_error(&format!(
            "{{\"now\":\"{NOW}\",\"placement\":{{\"at\":{{\"page\":-1,\"boundary\":{{\"x\":0.0,\"y\":0.0,\"w\":1.0,\"h\":1.0}}}}}}}}"
        )), "a page number that is not a u32");
        assert!(is_error("not json"));
        assert!(is_error(""), "nothing is not options");
        assert!(parse_verify_options("{}").is_ok());
        assert!(parse_verify_options("not json").is_err());
        assert!(parse_verify_options("{\"now\":5}").is_err());
        assert!(parse_verify_options("{\"certs\":{\"1\":\"not base64!\"}}").is_err());
    }

    #[test]
    fn report_json_has_stable_snake_case_fields() {
        let sample = fixture("SESV4SignDoc.ofd");
        let json = verify_json(&sample, "{}");
        let reports: Vec<serde_json::Value> = serde_json::from_str(&json).expect("a JSON array");
        assert_eq!(reports.len(), 1);
        let mut keys: Vec<&str> = reports[0]
            .as_object()
            .expect("an object")
            .keys()
            .map(String::as_str)
            .collect();
        keys.sort_unstable();
        assert_eq!(
            keys,
            vec![
                "certificate",
                "date_time",
                "id",
                "integrity",
                "kind",
                "overall",
                "provider",
                "seal_match",
                "signature_value",
                "variant",
            ]
        );
        // It reads back as what verification produced.
        let back: Vec<rofd_sign::SignatureReport> = serde_json::from_str(&json).expect("reports");
        assert_eq!(back, rofd_sign::verify(&sample, &VerifyOptions::default()));
        assert_eq!(back[0].overall, SignatureOverall::Valid);

        // The options: a time to judge the certificates by, and certificates
        // the package does not carry, by signature ID.
        let der = fixture("digital_signer_cert.der");
        let options = format!(
            "{{\"now\":\"{NOW}\",\"certs\":{{\"1\":\"{}\"}}}}",
            base64::engine::general_purpose::STANDARD.encode(&der)
        );
        let parsed = parse_verify_options(&options).expect("parse");
        assert_eq!(parsed.now.as_deref(), Some(NOW));
        assert_eq!(parsed.certs, vec![("1".to_owned(), der)]);
        assert_eq!(overalls(&verify_json(&sample, &options)), vec!["Valid"]);

        // What cannot be read yields no report, never an error.
        assert_eq!(verify_json(b"not a package", "{}"), "[]");
        assert_eq!(
            verify_json(&unsigned_package(), "{}"),
            "[]",
            "no signature yet"
        );
        assert_eq!(verify_json(&sample, "not json"), "[]");
    }

    #[test]
    fn signing_through_json_verifies() {
        let user_p12 = fixture("USER.p12");
        let signed = sign_with_pfx(
            &unsigned_package(),
            &user_p12,
            "777777",
            &seal_options_json(),
        )
        .expect("sign");
        let json = verify_json(&signed, "{}");
        assert_eq!(overalls(&json), vec!["Valid"]);
        let reports: Vec<serde_json::Value> = serde_json::from_str(&json).unwrap();
        assert_eq!(reports[0]["variant"], "SesV4");
        assert_eq!(reports[0]["kind"], "Seal");
        assert_eq!(reports[0]["certificate"]["subject"], "Test Certificate");

        // A digital signature.
        let signed = sign_with_pfx(
            &unsigned_package(),
            &user_p12,
            "777777",
            &format!("{{\"now\":\"{NOW}\"}}"),
        )
        .expect("sign");
        let json = verify_json(&signed, "{}");
        let reports: Vec<serde_json::Value> = serde_json::from_str(&json).unwrap();
        assert_eq!(reports[0]["variant"], "Gbt35275");
        assert_eq!(reports[0]["kind"], "Sign");

        // Failures are errors with a reason.
        let error = sign_with_pfx(
            &unsigned_package(),
            &user_p12,
            "wrong",
            &seal_options_json(),
        )
        .unwrap_err();
        assert!(
            matches!(error, rofd_sign::SignError::Password { .. }),
            "{error}"
        );
        let error =
            sign_with_pfx(&unsigned_package(), &user_p12, "777777", "not json").unwrap_err();
        assert!(error.to_string().contains("options"), "{error}");
    }

    #[test]
    fn certificate_info_json_matches_the_store_entry() {
        let user_p12 = fixture("USER.p12");
        let json = certificate_info_json(&user_p12, "777777").expect("entry");
        let mut store = CertificateStore::default();
        let entry = store.import(user_p12.clone(), "777777").expect("import");
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&json).unwrap(),
            serde_json::to_value(&entry).unwrap()
        );
        assert!(
            json.contains("\"subject_cn\":\"Test Certificate\""),
            "{json}"
        );
        let error = certificate_info_json(&user_p12, "wrong").unwrap_err();
        assert!(
            matches!(error, rofd_sign::SignError::Password { .. }),
            "{error}"
        );
        // A file that cannot be read is not reported as a wrong password.
        let error = certificate_info_json(b"not a certificate file", "x").unwrap_err();
        assert!(
            matches!(error, rofd_sign::SignError::Certificate { .. }),
            "{error}"
        );
    }

    #[test]
    fn seal_image_from_json_bytes_or_base64_is_the_same_picture() {
        let from_bytes = parse_sign_options(&seal_options_json())
            .unwrap()
            .seal
            .unwrap();
        let encoded = base64::engine::general_purpose::STANDARD.encode(png());
        let from_base64 = parse_sign_options(&format!(
            "{{\"now\":\"{NOW}\",\"seal\":{{\"png\":\"{encoded}\",\"sizeMm\":40.0}}}}"
        ))
        .unwrap()
        .seal
        .unwrap();
        assert_eq!(
            from_bytes,
            SealImage {
                png: png(),
                size_mm: 40.0
            }
        );
        assert_eq!(from_bytes, from_base64);
    }

    #[test]
    fn a_finished_step_carries_the_reports_as_json() {
        use rofd_component::SignStep as ComponentStep;
        let sample = fixture("SESV4SignDoc.ofd");
        let reports = rofd_sign::verify(&sample, &VerifyOptions::default());
        assert_eq!(reports.len(), 1);
        let StepOutcome::Landed(json) = step_result(ComponentStep::Done(reports.clone())).unwrap()
        else {
            panic!("a finished step lands the signature");
        };
        let back: Vec<SignatureReport> = serde_json::from_str(&json).expect("reports");
        assert_eq!(back, reports);
    }
}
