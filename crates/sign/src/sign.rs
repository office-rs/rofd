//! Creating a signature (spec 2026-11-23 §10): a seal or a digital signature
//! appended to a package.
//!
//! What the signature protects is fixed before any part of it exists: every
//! entry of the package except the signatures and the annotation layer.
//! Annotations can therefore be added afterwards without the signature
//! going invalid (GB/T 48666-2026, 6.3 f).
//!
//! With a seal picture the signer signs twice: the seal being made (its
//! `eSealInfo`), then the signature value (the to-be-signed data that holds
//! the seal and the digest of the signature description file). Without a
//! picture the signer signs once, the digest of the description file, which
//! becomes a GB/T 35275 signed message. [`prepare_sign`] and [`Prepared`]
//! expose the rounds one by one, for a signer that is not in this process (a
//! USB token, another machine); [`sign`] runs them in one call.

use rofd_dom::{OfdDocument, OfdWarning, Rect};
use rofd_io::zip_util::write_zip_borrowed;
use rofd_io::{parse_ofd, PackageHandle};

use crate::cert::parse_certificate;
use crate::error::SignError;
use crate::in_place::{plan_for, sign_bytes, InPlaceStep, PreparedInPlace, Regenerate, Seen};
use crate::package_util::{
    join, references, scaffolds, smallest_free_sign_index, Layout, ListRecord, Scaffold, Scaffolds,
    SignatureList, Unprotected,
};
use crate::ses_encoder::check_value;
use crate::signature_xml::{
    generalized_time, signature_xml, Reference, SignatureXmlInput, StampRef,
};
use crate::signer::Signer;

/// How many object IDs signing reserves in `MaxUnitID` for annotations added
/// afterwards (spec M14). A signed Document.xml can no longer change, so
/// the reserve is what later saves allocate from.
pub const SIGNATURE_ID_RESERVE: u64 = 65_536;
/// The long side of a seal whose size was not given.
pub const DEFAULT_SEAL_SIZE_MM: f64 = 40.0;
/// How far from the page edge a seal is placed when no margin was given.
pub const DEFAULT_MARGIN_MM: f64 = 10.0;
/// The largest seal picture that is embedded, counted after its conversion
/// to PNG. A picture that does not fit is refused, never written broken: it
/// travels inside every copy of the document and every reader decodes it.
pub const MAX_SEAL_PICTURE_BYTES: usize = 16 * 1024 * 1024;

// ---- what the caller asks for ----

/// Who signed, as the signature description records it.
#[derive(Debug, Clone, PartialEq)]
pub struct Provider {
    pub name: String,
    pub company: Option<String>,
    pub version: Option<String>,
}

impl Default for Provider {
    fn default() -> Self {
        Self {
            name: "rofd".to_owned(),
            company: None,
            version: None,
        }
    }
}

/// A seal picture: PNG bytes, or any image the `image` crate reads (it is
/// converted). `size_mm` is the long side; `0.0` means
/// [`DEFAULT_SEAL_SIZE_MM`].
#[derive(Debug, Clone, PartialEq)]
pub struct SealImage {
    pub png: Vec<u8>,
    pub size_mm: f64,
}

/// Which pages a seal goes on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pages {
    Last,
    All,
}

/// Where a seal goes.
#[derive(Debug, Clone, PartialEq)]
pub enum Placement {
    /// One page (numbered from 1) and a rectangle in the millimetres the
    /// editor shows the page in, counted from the page's top-left - what
    /// `SealPlacement::boundary` reports. It is written in page space,
    /// whose origin is the top-left corner of the physical box (GB/T 33190
    /// table 7); the two differ only on a page whose box does not start at
    /// 0 0. It must be finite and have a size.
    At { page: u32, boundary: Rect },
    /// The bottom right corner of each page, `right_mm` from the right edge
    /// and `bottom_mm` from the bottom.
    BottomRight {
        pages: Pages,
        right_mm: f64,
        bottom_mm: f64,
    },
}

/// Everything a signature needs besides the document and the signer.
#[derive(Debug, Clone, PartialEq)]
pub struct SignOptions {
    /// `None` for a digital signature: no picture, no placement.
    pub seal: Option<SealImage>,
    /// `None` means the last page's bottom right corner, 10 mm from each
    /// edge. Not consulted when there is no seal.
    pub placement: Option<Placement>,
    /// `YYYY-MM-DD HH:mm:ss`. The library never reads the clock.
    pub now: String,
    pub provider: Provider,
}

impl SignOptions {
    /// Options for a digital signature at `now`.
    pub fn new(now: impl Into<String>) -> Self {
        Self {
            seal: None,
            placement: None,
            now: now.into(),
            provider: Provider::default(),
        }
    }

    /// The placement in effect.
    pub fn placement(&self) -> Placement {
        self.placement.clone().unwrap_or(Placement::BottomRight {
            pages: Pages::Last,
            right_mm: DEFAULT_MARGIN_MM,
            bottom_mm: DEFAULT_MARGIN_MM,
        })
    }
}

/// Sign a package: append a seal or a digital signature and return the
/// signed package. The input is not changed.
///
/// Bytes in, bytes out, for a caller that has no document open (a command
/// line, a binding). The signature is made the way
/// [`in_place::sign_in_place`](crate::in_place::sign_in_place) makes
/// it, on the package the parsed document carries, and the package is zipped
/// at the end. One thing differs from an open document: a package that
/// cannot take a required patch to Document.xml is refused here, where a
/// document is given a generated package first.
pub fn sign(ofd: &[u8], signer: &mut dyn Signer, opts: &SignOptions) -> Result<Vec<u8>, SignError> {
    let mut report = parse_ofd(ofd)?;
    sign_bytes(&mut report.document, signer, opts)?;
    zipped(report.document.package.as_ref())
}

/// Do everything up to the first signature a signer has to produce, for a
/// package as bytes: the two-phase form of [`sign`].
///
/// `cert_der` is the signer's certificate: it is embedded in the seal and
/// in the signature value, and every signature handed to [`Prepared::apply`]
/// must verify against it.
pub fn prepare_sign(
    ofd: &[u8],
    cert_der: &[u8],
    opts: &SignOptions,
) -> Result<Prepared, SignError> {
    let report = parse_ofd(ofd)?;
    let step = PreparedInPlace::first(plan_for(
        &report.document,
        cert_der,
        opts,
        Regenerate::Never,
    )?)?;
    Ok(Prepared {
        document: Box::new(report.document),
        step,
    })
}

/// What comes out of [`Prepared::apply`]: the next round to sign, or the
/// signed package.
#[derive(Debug)]
pub enum SignStep {
    More(Prepared),
    Done(Vec<u8>),
}

/// A package with everything a signature needs but the signature itself, as
/// bytes: the two-phase form for a signer that is not in this process (a
/// USB token, another machine). The document parsed from the package goes
/// along, so the rounds are made on the package it carries and the signed
/// package is zipped from it at the end.
///
/// [`Self::to_sign`] is what the signer signs; [`Self::apply`] takes that
/// signature and either yields the next round or the signed package.
#[derive(Debug)]
pub struct Prepared {
    /// Boxed: a document is large, and the step enum is passed around by
    /// value.
    document: Box<OfdDocument>,
    step: PreparedInPlace,
}

impl Prepared {
    /// What the signer signs in this round.
    pub fn to_sign(&self) -> &[u8] {
        self.step.to_sign()
    }

    /// Take the signature of [`Self::to_sign`] and go on: the round that
    /// produces the signature value, or the signed package. A signature
    /// that does not verify with the signer's certificate is not written.
    pub fn apply(self, signature: &[u8]) -> Result<SignStep, SignError> {
        match self.step.apply(signature)? {
            InPlaceStep::More(step) => Ok(SignStep::More(Prepared {
                document: self.document,
                step,
            })),
            InPlaceStep::Done(appendix) => {
                let mut document = self.document;
                appendix.land(&mut document, false)?;
                Ok(SignStep::Done(zipped(document.package.as_ref())?))
            }
        }
    }
}

/// A package handle back as a file, entry for entry and in its order.
fn zipped(package: Option<&PackageHandle>) -> Result<Vec<u8>, SignError> {
    let package = package.ok_or_else(|| invalid("the package declares no document"))?;
    let borrowed: Vec<(&str, &[u8])> = package
        .entries
        .iter()
        .map(|entry| (entry.name.as_str(), entry.bytes.as_slice()))
        .collect();
    Ok(write_zip_borrowed(&borrowed)?)
}

// ---- what is fixed before the first round ----

/// Everything of a signature except what the signer produces, planned over
/// one package (spec 2026-11-23 §10.2).
///
/// The plan copies no entry of the package: the references are digests, and
/// the scaffolds are the few entries the first signature of a package
/// replaces or adds. So a plan is small, and the package is read where it
/// lies. [`Plan::seen`] keeps a shared handle on every protected entry -
/// the buffers themselves, not copies - so that the package is recognised
/// again when the signature lands.
#[derive(Debug)]
pub(crate) struct Plan {
    /// What the first signature of a package writes besides its own files.
    pub scaffolds: Option<Scaffolds>,
    /// The protected entries, by path; the seal is added when it exists.
    pub references: Vec<Reference>,
    pub provider: Provider,
    pub now: String,
    /// The new record of the signature list.
    pub record: ListRecord,
    pub max_sign_id: String,
    /// Package directory of the new signature, e.g. `Doc_0/Signs/Sign_1`.
    pub dir: String,
    pub list_path: String,
    /// The list as the package held it; its records are kept as written.
    pub list: SignatureList,
    pub stamps: Vec<Stamp>,
    /// The seal picture as it will be written, when there is one: the
    /// first round makes the seal of it.
    pub picture: Option<Picture>,
    /// The signer's certificate, embedded in what is written.
    pub certificate: Vec<u8>,
    /// Its public key: every signature is checked against it before it is
    /// written.
    pub public_key: Vec<u8>,
    /// The package made for a document that had none it could sign; handed
    /// to the document when the signature lands.
    pub package: Option<PackageHandle>,
    /// The document as it was when the plan was made: what the signature is
    /// checked against when it lands (`Seen`).
    pub seen: Seen,
    /// What reading back a package made for the document warned about
    /// (`Plan::package`); handed to the caller when the signature lands.
    pub warnings: Vec<OfdWarning>,
}

/// Where a seal appears on one page.
#[derive(Debug, Clone)]
pub(crate) struct Stamp {
    pub id: u64,
    /// The page ID as Document.xml writes it.
    pub page_id: String,
    /// Page-local millimetres.
    pub boundary: Rect,
}

/// Plan a signature on `package`, the one `document` is saved with.
///
/// `package` may be one made for the document (`Plan::package`); `document`
/// is what the placement and the ID reserve are computed from. `seen` is
/// the document as it is now: a plan lands only on the document it was
/// planned on.
pub(crate) fn plan(
    package: &PackageHandle,
    document: &OfdDocument,
    certificate_der: &[u8],
    opts: &SignOptions,
    seen: Seen,
) -> Result<Plan, SignError> {
    let certificate = certificate_der.to_vec();
    let public_key = parse_certificate(&certificate)
        .map_err(|_| invalid("the signer certificate is not an X.509 certificate"))?
        .public_key_sec1;
    generalized_time(&opts.now)?;

    let layout = Layout::of(package)?;
    // Only a package that is not signed yet can still receive the scaffolds.
    let scaffolds = if layout.list.records.is_empty() {
        match scaffolds(package, document, &layout)? {
            Scaffold::Ready(scaffolds) => Some(scaffolds),
            // Reported as it always was. A document that is signed in place
            // is given a generated package before it gets here.
            Scaffold::NoAnchor(error) => return Err(error),
        }
    } else {
        None
    };
    // The entries the signature is computed over: the package's own, with
    // a scaffold's patch where it replaces one. Borrowed; nothing is copied.
    let entries: Vec<(&str, &[u8])> = package
        .entries
        .iter()
        .map(|entry| {
            let patched = scaffolds
                .iter()
                .flat_map(|scaffolds| scaffolds.patches.iter())
                .find(|patch| patch.name == entry.name);
            match patched {
                Some(patch) => (entry.name.as_str(), patch.bytes.as_slice()),
                None => (entry.name.as_str(), entry.bytes.as_slice()),
            }
        })
        .collect();
    let index = smallest_free_sign_index(entries.iter().map(|(name, _)| *name), &layout.sign_dir);
    let dir = join(&layout.sign_dir, &format!("Sign_{index}"));
    let picture = opts.seal.as_ref().map(picture_of).transpose()?;
    let placement = opts.placement();
    let (signature_id, stamps) = stamp_plan(document, &placement, picture.as_ref(), &layout.list)?;

    let record = ListRecord {
        id: signature_id.to_string(),
        kind: Some(if picture.is_some() { "Seal" } else { "Sign" }.to_owned()),
        base_loc: format!("/{dir}/Signature.xml"),
    };
    let max_sign_id = stamps
        .last()
        .map_or(signature_id, |stamp| stamp.id)
        .to_string();
    let references = references(&entries, &Unprotected::of(package, &layout, &dir));
    Ok(Plan {
        scaffolds,
        references,
        provider: opts.provider.clone(),
        now: opts.now.clone(),
        record,
        max_sign_id,
        dir,
        list_path: layout.list_path,
        list: layout.list,
        stamps,
        picture,
        certificate,
        public_key,
        package: None,
        seen,
        warnings: Vec::new(),
    })
}

impl Plan {
    pub(crate) fn file_loc(&self, name: &str) -> String {
        format!("/{}/{}", self.dir, name)
    }

    /// The signature description file (Signature.xml). `seal` is the
    /// finished seal, when there is one: the package's copy of it is
    /// protected as well.
    pub(crate) fn description(&self, seal: Option<&[u8]>) -> String {
        let mut references = self.references.clone();
        if let Some(seal) = seal {
            references.push(Reference {
                file_ref: self.file_loc("Seal.esl"),
                check_value_base64: check_value(seal),
            });
        }
        let stamps: Vec<StampRef<'_>> = self
            .stamps
            .iter()
            .map(|stamp| StampRef {
                id: stamp.id,
                page_id: &stamp.page_id,
                boundary: stamp.boundary,
            })
            .collect();
        signature_xml(&SignatureXmlInput {
            signed_value_loc: &self.file_loc("SignedValue.dat"),
            provider_name: &self.provider.name,
            provider_company: self.provider.company.as_deref(),
            provider_version: self.provider.version.as_deref(),
            date_time: &self.now,
            seal_base_loc: seal.map(|_| self.file_loc("Seal.esl")).as_deref(),
            stamps: &stamps,
            references: &references,
        })
    }
}

/// A seal picture ready for the package.
#[derive(Debug, Clone)]
pub(crate) struct Picture {
    /// The picture as an 8-bit RGBA PNG, whatever format it came in.
    pub(crate) png: Vec<u8>,
    pub(crate) width_mm: f64,
    pub(crate) height_mm: f64,
}

/// Read a seal picture: its size in millimetres, and its bytes as a PNG.
///
/// The long side is `size_mm`; the other side follows the pixel ratio.
fn picture_of(seal: &SealImage) -> Result<Picture, SignError> {
    picture_within(seal, MAX_SEAL_PICTURE_BYTES)
}

/// [`picture_of`] with the largest PNG it takes. The limit counts what is
/// written into the seal: the picture as a PNG, whatever it came as.
fn picture_within(seal: &SealImage, max_png_bytes: usize) -> Result<Picture, SignError> {
    let size_mm = if seal.size_mm == 0.0 {
        DEFAULT_SEAL_SIZE_MM
    } else {
        seal.size_mm
    };
    if !size_mm.is_finite() || size_mm <= 0.0 {
        return Err(invalid(
            "the seal size must be a positive number of millimetres",
        ));
    }
    let image = image::load_from_memory(&seal.png)
        .map_err(|_| invalid("the seal picture cannot be decoded"))?;
    let (pixels_w, pixels_h) = (f64::from(image.width()), f64::from(image.height()));
    let scale = size_mm / pixels_w.max(pixels_h);
    let (width_mm, height_mm) = (mm(pixels_w * scale), mm(pixels_h * scale));
    if width_mm < 1.0 || height_mm < 1.0 {
        return Err(invalid(
            "the seal picture is too elongated: one side would be shorter than a millimetre",
        ));
    }
    let mut png = std::io::Cursor::new(Vec::new());
    image
        .to_rgba8()
        .write_to(&mut png, image::ImageFormat::Png)
        .map_err(|_| SignError::Encode {
            reason: "the seal picture cannot be written as PNG".to_owned(),
        })?;
    let png = png.into_inner();
    if png.len() > max_png_bytes {
        return Err(invalid(format!(
            "the seal picture is too large: {:.1} MiB as PNG, the limit is {:.1} MiB; choose a smaller picture",
            png.len() as f64 / 1_048_576.0,
            max_png_bytes as f64 / 1_048_576.0
        )));
    }
    Ok(Picture {
        png,
        width_mm,
        height_mm,
    })
}

/// Millimetres to a thousandth: what is computed carries no float noise.
fn mm(value: f64) -> f64 {
    (value * 1000.0).round() / 1000.0
}

/// The ID of the new signature and where its seal goes.
fn stamp_plan(
    document: &OfdDocument,
    placement: &Placement,
    picture: Option<&Picture>,
    list: &SignatureList,
) -> Result<(u64, Vec<Stamp>), SignError> {
    let signature_id = next_signature_id(list)?;
    let Some(picture) = picture else {
        return Ok((signature_id, Vec::new()));
    };
    let stamps = places_of(document, placement, picture)?
        .into_iter()
        .enumerate()
        .map(|(offset, (index, boundary))| Stamp {
            // The stamps take the IDs after the signature's.
            id: signature_id + 1 + offset as u64,
            page_id: document.pages[index].id.0.clone(),
            boundary,
        })
        .collect();
    Ok((signature_id, stamps))
}

/// The next free signature ID: above the largest numeric `MaxSignId` and
/// `Signature@ID` of the list. IDs that are not numbers do not count.
fn next_signature_id(list: &SignatureList) -> Result<u64, SignError> {
    let numeric = |text: &str| text.trim().parse::<u64>().ok();
    let largest = list
        .max_sign_id
        .as_deref()
        .and_then(numeric)
        .into_iter()
        .chain(list.records.iter().filter_map(|record| numeric(&record.id)))
        .max()
        .unwrap_or(0);
    largest
        .checked_add(1)
        .ok_or_else(|| invalid("the signature IDs are used up"))
}

/// `(page index, boundary)` for every page the seal goes on.
fn places_of(
    document: &OfdDocument,
    placement: &Placement,
    picture: &Picture,
) -> Result<Vec<(usize, Rect)>, SignError> {
    match placement {
        Placement::At { page, boundary } => {
            let index = usize::try_from(*page)
                .ok()
                .filter(|page| (1..=document.pages.len()).contains(page))
                .ok_or_else(|| {
                    invalid(format!(
                        "page {page} of {} does not exist",
                        document.pages.len()
                    ))
                })?;
            if ![boundary.x, boundary.y, boundary.w, boundary.h]
                .iter()
                .all(|value| value.is_finite())
                || boundary.w <= 0.0
                || boundary.h <= 0.0
            {
                return Err(invalid(
                    "the seal boundary must be a rectangle of finite millimetres with a size",
                ));
            }
            // The boundary comes in the millimetres the editor shows the
            // page in, from its top-left. A boundary is written in page
            // space, whose origin is the top-left corner of the physical
            // box (GB/T 33190 table 7); the automatic placement counts from
            // there too. The two are the same on a page whose box starts at
            // 0 0.
            let page_box = &document.pages[index - 1].physical_box;
            let in_page_space = Rect {
                x: page_box.x + boundary.x,
                y: page_box.y + boundary.y,
                w: boundary.w,
                h: boundary.h,
            };
            Ok(vec![(index - 1, in_page_space)])
        }
        Placement::BottomRight {
            pages,
            right_mm,
            bottom_mm,
        } => bottom_right(document, *pages, *right_mm, *bottom_mm, picture),
    }
}

/// The bottom right corner of each page of `pages`: `right_mm` from the
/// right edge and `bottom_mm` from the bottom, counted from the page's
/// physical box. A seal that would start outside the page is refused.
fn bottom_right(
    document: &OfdDocument,
    pages: Pages,
    right_mm: f64,
    bottom_mm: f64,
    picture: &Picture,
) -> Result<Vec<(usize, Rect)>, SignError> {
    if ![right_mm, bottom_mm]
        .iter()
        .all(|margin| margin.is_finite() && *margin >= 0.0)
    {
        return Err(invalid("the seal margins must be zero or more millimetres"));
    }
    let last = document
        .pages
        .len()
        .checked_sub(1)
        .ok_or_else(|| invalid("the document has no page to place a seal on"))?;
    let indexes: Vec<usize> = match pages {
        Pages::Last => vec![last],
        Pages::All => (0..document.pages.len()).collect(),
    };
    let Picture {
        width_mm,
        height_mm,
        ..
    } = picture;
    indexes
        .into_iter()
        .map(|index| {
            let page_box = document.pages[index].physical_box;
            let x = page_box.x + page_box.w - right_mm - width_mm;
            let y = page_box.y + page_box.h - bottom_mm - height_mm;
            // The page starts where its box does, not at 0.
            if !x.is_finite() || !y.is_finite() || x < page_box.x || y < page_box.y {
                return Err(invalid(format!(
                    "page {} has no room for a {width_mm} x {height_mm} mm seal \
                     with margins of {right_mm} mm on the right and {bottom_mm} mm at the bottom",
                    index + 1
                )));
            }
            Ok((
                index,
                Rect {
                    x: mm(x),
                    y: mm(y),
                    w: *width_mm,
                    h: *height_mm,
                },
            ))
        })
        .collect()
}

pub(crate) fn invalid(reason: impl Into<String>) -> SignError {
    SignError::InvalidInput {
        reason: reason.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rgba(width: u32, height: u32) -> Vec<u8> {
        let mut png = std::io::Cursor::new(Vec::new());
        image::RgbaImage::from_pixel(width, height, image::Rgba([1, 2, 3, 255]))
            .write_to(&mut png, image::ImageFormat::Png)
            .expect("PNG");
        png.into_inner()
    }

    fn seal(png: Vec<u8>, size_mm: f64) -> SealImage {
        SealImage { png, size_mm }
    }

    fn list(ids: &[&str], max_sign_id: Option<&str>) -> SignatureList {
        SignatureList {
            records: ids
                .iter()
                .map(|id| ListRecord {
                    id: (*id).to_owned(),
                    kind: Some("Seal".to_owned()),
                    base_loc: String::new(),
                })
                .collect(),
            max_sign_id: max_sign_id.map(str::to_owned),
        }
    }

    #[test]
    fn a_picture_gets_its_size_from_the_long_side_and_the_pixel_ratio() {
        let picture = picture_of(&seal(rgba(80, 40), 40.0)).expect("picture");
        assert_eq!((picture.width_mm, picture.height_mm), (40.0, 20.0));
        assert!(
            picture.png.starts_with(&[0x89, b'P', b'N', b'G']),
            "written as PNG"
        );
        // No size given: 40 mm.
        assert_eq!(picture_of(&seal(rgba(40, 40), 0.0)).unwrap().width_mm, 40.0);
        // Ratios that do not divide evenly are cut at a thousandth of a millimetre.
        let picture = picture_of(&seal(rgba(91, 37), 40.0)).unwrap();
        assert_eq!((picture.width_mm, picture.height_mm), (40.0, 16.264));
    }

    #[test]
    fn an_unusable_picture_or_size_is_refused() {
        let is_invalid = |result: Result<Picture, SignError>| {
            matches!(result, Err(SignError::InvalidInput { .. }))
        };
        assert!(is_invalid(picture_of(&seal(
            b"not an image".to_vec(),
            40.0
        ))));
        assert!(is_invalid(picture_of(&seal(Vec::new(), 40.0))));
        assert!(is_invalid(picture_of(&seal(rgba(40, 40), -1.0))));
        assert!(is_invalid(picture_of(&seal(rgba(40, 40), f64::NAN))));
        assert!(is_invalid(picture_of(&seal(rgba(40, 40), f64::INFINITY))));
        assert!(
            is_invalid(picture_of(&seal(rgba(4000, 1), 40.0))),
            "a tenth of a millimetre is no seal"
        );
    }

    /// Incompressible pixels, so the PNG is about as large as the raw picture.
    fn noise(width: u32, height: u32) -> Vec<u8> {
        let image = image::RgbaImage::from_fn(width, height, |x, y| {
            let mixed = x.wrapping_mul(2_654_435_761) ^ y.wrapping_mul(224_682_251);
            image::Rgba(mixed.to_le_bytes())
        });
        let mut png = std::io::Cursor::new(Vec::new());
        image
            .write_to(&mut png, image::ImageFormat::Png)
            .expect("PNG");
        png.into_inner()
    }

    #[test]
    fn a_seal_picture_over_the_limit_is_refused() {
        // Final review C2: a picture past the limit used to be embedded with
        // a truncated DER length, which no reader could parse.
        let png = noise(300, 300);
        assert!(png.len() > 100_000, "{} bytes", png.len());
        match picture_within(&seal(png, 40.0), 100_000) {
            Err(SignError::InvalidInput { reason }) => {
                assert!(reason.contains("too large"), "{reason}")
            }
            other => panic!("refused as an invalid input, got {other:?}"),
        }
        // The limit itself, not the number of pixels: flat pixels compress
        // to a small picture, which is taken.
        assert!(picture_within(&seal(rgba(300, 300), 40.0), 100_000).is_ok());
        // And the real limit takes what it was sized for.
        assert!(picture_of(&seal(noise(300, 300), 40.0)).is_ok());
    }

    #[test]
    fn the_next_signature_id_is_above_every_numeric_id() {
        assert_eq!(next_signature_id(&list(&[], None)).unwrap(), 1);
        assert_eq!(next_signature_id(&list(&["1"], Some("2"))).unwrap(), 3);
        assert_eq!(next_signature_id(&list(&["7"], None)).unwrap(), 8);
        assert_eq!(
            next_signature_id(&list(&["s001", "s002"], Some("s1"))).unwrap(),
            1
        );
        assert_eq!(
            next_signature_id(&list(&["s001", "5"], Some("s1"))).unwrap(),
            6
        );
        let largest = u64::MAX.to_string();
        let used_up = list(&[], Some(largest.as_str()));
        assert!(next_signature_id(&used_up).is_err());
    }

    #[test]
    fn millimetres_carry_no_float_noise() {
        assert_eq!(mm(210.0 - 10.0 - 46.27), 153.73);
        assert_eq!(mm(160.0), 160.0);
    }
}
