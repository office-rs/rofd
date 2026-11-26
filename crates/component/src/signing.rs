//! Signing from the component (spec 2026-11-23 §17.11, §17.13): the signature
//! goes into the document that is open, and the component takes care of what
//! follows - the page caches, the modified flag, the print sessions, the
//! host. Verifying the open document is here as well: an exit of the library
//! offers both or neither.
//!
//! Behind the `sign` feature: only a host that signs needs rofd-sign, and
//! the editor kernel does not make every user of it compile the
//! cryptography.
//!
//! The types a host meets here are rofd-sign's, re-exported, so that a host
//! that signs and verifies through the component need not name rofd-sign.
#![cfg(feature = "sign")]

pub use rofd_dom::SignatureKind;
pub use rofd_sign::{
    newly_invalidated, summarize, CertificateReport, IntegrityFailure, IntegrityResult, Pages,
    Placement, Provider, SealImage, SignError, SignOptions, SignatureOverall, SignatureReport,
    SignatureSummary, SignatureValue, SignatureValueStatus, SignatureVariant, Signer,
    VerifyOptions,
};
use rofd_sign::{Landed, PfxSigner, PreparedInPlace};

use crate::ofd_component::OfdComponent;

/// A signature prepared on this component's document and not in it yet:
/// for a signer that is not in this process. [`Self::to_sign`] is what the
/// signer signs; the signature goes back in through
/// [`OfdComponent::finish_sign`], which alone reaches the document.
pub struct PreparedSignature(pub(crate) PreparedInPlace);

impl PreparedSignature {
    /// What the signer signs in this round.
    pub fn to_sign(&self) -> &[u8] {
        self.0.to_sign()
    }
}

/// What comes out of [`OfdComponent::finish_sign`]: the next round to sign,
/// or the reports of every signature of the document afterwards.
pub enum SignStep {
    More(PreparedSignature),
    Done(Vec<SignatureReport>),
}

impl OfdComponent {
    /// Append a signature to the document that is open (spec 2026-11-23
    /// §17.11): a seal, or a digital signature without a picture.
    ///
    /// The seal is on its page at the next frame. Nothing else moves - the
    /// viewport, the selection, the text being edited and the undo history
    /// are those of the document the user was working on; the signature is
    /// no edit of the annotations and does not enter the history. The
    /// document counts as modified: the file does not carry the signature
    /// until the host saves it, which is an ordinary save.
    ///
    /// Refused, with nothing changed, when the document's file cannot take
    /// its annotations ([`Self::sign_refusal`]) or an option is unusable.
    /// Returns the reports of every signature of the document afterwards;
    /// what the landing has to warn about goes to the `on_warning` handler.
    pub fn sign(
        &mut self,
        signer: &mut dyn Signer,
        opts: &SignOptions,
    ) -> Result<Vec<SignatureReport>, SignError> {
        let landed = self
            .editor
            .revise_document(|document| rofd_sign::sign_in_place(document, signer, opts))?;
        self.signature_landed(&landed);
        Ok(landed.reports)
    }

    /// [`Self::sign`] with a certificate file (`.pfx`, `.p12`) and its
    /// password: the signer is opened from the file, signs, and is dropped
    /// with it. A wrong password is [`SignError::Password`]; a file that
    /// cannot be used at all is [`SignError::Certificate`].
    pub fn sign_pfx(
        &mut self,
        pfx: &[u8],
        password: &str,
        opts: &SignOptions,
    ) -> Result<Vec<SignatureReport>, SignError> {
        let mut signer = PfxSigner::open(pfx, password)?;
        self.sign(&mut signer, opts)
    }

    /// Verify every signature of the document that is open (spec 2026-11-23
    /// §17.13): what the document carries now, saved or not. No file is read
    /// and nothing is saved for it. Verification never fails, it reports -
    /// one report per signature, in list order, whatever state their files
    /// are in.
    pub fn verify_signatures(&self, opts: &VerifyOptions) -> Vec<SignatureReport> {
        rofd_sign::verify_document(self.editor.document(), opts)
    }

    /// What stops a signature of the document right now, before anything is
    /// made: its file declares signatures and declares no annotation entry,
    /// and the document has annotations to write. A host asks this first to
    /// tell the user in its own words.
    pub fn sign_refusal(&self) -> Result<(), SignError> {
        rofd_sign::sign_refusal(self.editor.document())
    }

    /// Start a signature for a signer that is not in this process: the
    /// two-phase form of [`Self::sign`]. `certificate_der` is the signer's
    /// certificate. Preparing changes nothing of the document.
    pub fn prepare_sign(
        &self,
        certificate_der: &[u8],
        opts: &SignOptions,
    ) -> Result<PreparedSignature, SignError> {
        rofd_sign::prepare_in_place(self.editor.document(), certificate_der, opts)
            .map(PreparedSignature)
    }

    /// Take the signature of [`PreparedSignature::to_sign`] and go on: the
    /// next round, or - once the signature is complete - the reports of
    /// every signature of the document, the signature being in the document
    /// then, with everything that [`Self::sign`] does after one.
    pub fn finish_sign(
        &mut self,
        prepared: PreparedSignature,
        signature: &[u8],
    ) -> Result<SignStep, SignError> {
        Ok(match prepared.0.apply(signature)? {
            rofd_sign::InPlaceStep::More(next) => SignStep::More(PreparedSignature(next)),
            rofd_sign::InPlaceStep::Done(appendix) => {
                let landed = self
                    .editor
                    .revise_document(|document| appendix.append_to(document))?;
                self.signature_landed(&landed);
                SignStep::Done(landed.reports)
            }
        })
    }

    /// What follows a signature in the document: its warnings reach the host
    /// through the warning callback, like the ones of a load - one batch when
    /// there is something to say and no call when there is not, since a host
    /// that keeps the last batch would read an empty one as "no warnings";
    /// the seals are page content, so every cached page is stale; and it is
    /// a change like any edit - the document is modified, print sessions are
    /// stale and the host is told.
    fn signature_landed(&mut self, landed: &Landed) {
        if !landed.warnings.is_empty() {
            self.fire_warnings(&landed.warnings);
        }
        self.editor.invalidate_pages();
        self.notify_document_change();
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use imaging::record::{Command, Draw};

    use crate::render_target::RenderTarget;
    use crate::{OfdComponent, OfdConfig, ViewEvent};
    use rofd_dom::{
        AnnotationKind, AnnotationPayload, AnnotationSelection, Color, Layer, LayerType,
        OfdWarning, Page, PageId, Rect, ShapeKind,
    };
    use rofd_sign::{SignError, SignOptions, SignatureOverall, Signer, TestCa, VerifyOptions};

    use super::SignStep;

    const NOW: &str = "2026-11-24 10:30:00";

    /// A render target that keeps the last scene it was handed.
    struct Capture(rofd_render::Scene);

    impl RenderTarget for Capture {
        fn draw_scene(&mut self, scene: &rofd_render::Scene) -> bool {
            self.0 = scene.clone();
            true
        }
        fn size(&self) -> (f64, f64) {
            (200.0, 200.0)
        }
    }

    /// The placement transforms of the image-brush fills in `scene`: where
    /// the seals are.
    fn image_placements(scene: &rofd_render::Scene) -> Vec<imaging::kurbo::Affine> {
        scene
            .commands()
            .iter()
            .filter_map(|cmd| match cmd {
                Command::Draw(id) => match scene.draw_op(*id) {
                    Draw::Fill {
                        brush: imaging::peniko::Brush::Image(_),
                        transform,
                        ..
                    } => Some(*transform),
                    _ => None,
                },
                _ => None,
            })
            .collect()
    }

    fn seal_png() -> Vec<u8> {
        let image = image::RgbaImage::from_pixel(40, 40, image::Rgba([190, 30, 30, 255]));
        let mut bytes = std::io::Cursor::new(Vec::new());
        image
            .write_to(&mut bytes, image::ImageFormat::Png)
            .expect("PNG");
        bytes.into_inner()
    }

    fn page(id: &str) -> Page {
        Page {
            raw_children: Vec::new(),
            id: PageId::new(id),
            physical_box: Rect {
                x: 0.0,
                y: 0.0,
                w: 200.0,
                h: 200.0,
            },
            layers: vec![Layer {
                layer_type: LayerType::Body,
                objects: Vec::new(),
                raw_children: Vec::new(),
                raw_attrs: Vec::new(),
            }],
            template: None,
        }
    }

    /// A component with two pages that came from a file, the way a host
    /// opens one: 200 x 200 mm pages, viewport 200 x 200, zoom 1.
    fn component_with_package() -> OfdComponent {
        let mut document = rofd_dom::OfdDocument::default();
        document.pages.push(page("P0"));
        document.pages.push(page("P1"));
        let bytes = rofd_io::write_ofd(&document).expect("the package");
        let report = rofd_io::parse_ofd(&bytes).expect("parse");
        let mut c = OfdComponent::new_native(OfdConfig::new(Arc::new(vec![])));
        c.set_clock("t".into(), 1);
        c.load_document(report.document);
        c.viewport = rofd_render::Viewport {
            scroll: (0.0, 0.0),
            zoom: 1.0,
            size: (200.0, 200.0),
            page_gap: 0.0,
        };
        c
    }

    /// A seal of 40 x 40 mm at (20, 20) on `page` (from 1).
    fn seal_options(page: u32) -> SignOptions {
        SignOptions {
            seal: Some(rofd_sign::SealImage {
                png: seal_png(),
                size_mm: 40.0,
            }),
            placement: Some(rofd_sign::Placement::At {
                page,
                boundary: Rect {
                    x: 20.0,
                    y: 20.0,
                    w: 40.0,
                    h: 40.0,
                },
            }),
            ..SignOptions::new(NOW)
        }
    }

    fn rect() -> AnnotationPayload {
        AnnotationPayload::Shape {
            kind: ShapeKind::Rect,
            rect: Rect {
                x: 10.0,
                y: 10.0,
                w: 50.0,
                h: 30.0,
            },
            stroke: Color::Rgb(255, 0, 0),
            fill: None,
            width: 1.0,
            points: vec![],
        }
    }

    fn signer() -> rofd_sign::TestSigner {
        TestCa::from_seed("rofd test signer", b"seed-a").signer()
    }

    #[test]
    fn signing_keeps_the_view_the_selection_and_the_history_and_shows_the_seal() {
        let mut c = component_with_package();
        c.handle_event(&ViewEvent::Scroll { dx: 0.0, dy: 200.0 });
        let edit = c.create_annotation(
            AnnotationKind::Shape(ShapeKind::Rect),
            PageId::new("P1"),
            rect(),
        );
        let (scroll, page, zoom) = (c.viewport.scroll, c.current_page, c.viewport.zoom);
        assert_eq!(page, Some(1), "on the second page");
        let changes = Arc::new(Mutex::new(0));
        let counted = changes.clone();
        c.on_change(move |_| *counted.lock().unwrap() += 1);
        c.clear_modified();
        let mut target = Capture(rofd_render::Scene::default());
        c.render(&mut target);
        assert!(image_placements(&target.0).is_empty(), "unsigned: no seal");

        let reports = c.sign(&mut signer(), &seal_options(2)).expect("sign");

        assert_eq!(reports.len(), 1);
        assert_eq!(reports[0].overall, SignatureOverall::Valid);
        assert_eq!(c.document().signatures.items.len(), 1);
        assert_eq!(c.viewport.scroll, scroll, "the view stays where it was");
        assert_eq!(c.current_page, page);
        assert_eq!(c.viewport.zoom, zoom);
        assert!(c.can_undo(), "the edit can still be undone");
        assert!(
            matches!(c.selection(), AnnotationSelection::Single(selected) if *selected == edit)
        );
        assert!(c.is_modified(), "the file does not carry the signature yet");
        assert_eq!(c.body_revision(), 1);
        assert_eq!(*changes.lock().unwrap(), 1, "the host hears of the change");
        c.render(&mut target);
        assert_eq!(
            image_placements(&target.0).len(),
            1,
            "the seal is on the page that was already drawn"
        );
    }

    #[test]
    fn a_failed_signature_changes_nothing() {
        let mut c = component_with_package();
        c.clear_modified();
        let error = c.sign(&mut signer(), &seal_options(9)).unwrap_err();
        assert!(matches!(error, SignError::InvalidInput { .. }), "{error}");
        assert!(c.document().signatures.items.is_empty());
        assert!(!c.is_modified());
        assert_eq!(c.body_revision(), 0);
    }

    #[test]
    fn preparing_changes_nothing_and_finishing_lands_the_signature() {
        let mut c = component_with_package();
        let ca = TestCa::from_seed("rofd test signer", b"seed-a");
        let prepared = c
            .prepare_sign(&ca.cert_der, &SignOptions::new(NOW))
            .expect("prepare");
        assert!(c.document().signatures.items.is_empty());
        assert!(!c.is_modified());
        let signature = ca.signer().sign(prepared.to_sign()).expect("sign");
        let SignStep::Done(reports) = c.finish_sign(prepared, &signature).expect("one round")
        else {
            panic!("a digital signature takes one round");
        };
        assert_eq!(reports.len(), 1);
        assert_eq!(c.document().signatures.items.len(), 1);
        assert!(c.is_modified());
        assert_eq!(c.body_revision(), 1);
    }

    /// A document that was never saved: no package behind it.
    fn unsaved_document() -> rofd_dom::OfdDocument {
        let mut document = rofd_dom::OfdDocument::default();
        document.pages.push(page("P0"));
        document
    }

    /// A component whose document was never saved.
    fn component_without_package() -> OfdComponent {
        let mut c = OfdComponent::new_native(OfdConfig::new(Arc::new(vec![])));
        c.set_clock("t".into(), 1);
        c.load_document(unsaved_document());
        c
    }

    #[test]
    fn an_annotation_made_between_the_phases_does_not_stop_the_signature_of_an_unsaved_document() {
        // The editor raises MaxUnitID as it numbers an annotation on an
        // unsigned document. That is still only an edit of the annotations.
        let mut c = component_without_package();
        let ca = TestCa::from_seed("rofd test signer", b"seed-a");
        let prepared = c
            .prepare_sign(&ca.cert_der, &SignOptions::new(NOW))
            .expect("prepare");
        let signature = ca.signer().sign(prepared.to_sign()).expect("sign");
        let page_id = c.document().pages[0].id.clone();
        c.create_annotation(AnnotationKind::Shape(ShapeKind::Rect), page_id, rect());
        assert_eq!(c.document().max_unit_id, 1, "the editor moved MaxUnitID");
        let SignStep::Done(reports) = c.finish_sign(prepared, &signature).expect("land") else {
            panic!("a digital signature takes one round");
        };
        assert_eq!(reports.len(), 1);
        assert_eq!(reports[0].overall, SignatureOverall::Valid);
        assert_eq!(c.document().signatures.items.len(), 1);
        assert_eq!(c.document().annotations.by_page.len(), 1, "the edit stays");
        assert!(c.is_modified());
    }

    #[test]
    fn a_signature_prepared_on_a_document_does_not_land_on_the_next_one() {
        // The two-phase form across a document swap: the signature was made
        // for the document that was open, and lands on no other. Two
        // documents that were never saved have no package to tell them
        // apart; what a package would be generated from does.
        let mut c = component_without_package();
        let ca = TestCa::from_seed("rofd test signer", b"seed-a");
        let prepared = c
            .prepare_sign(&ca.cert_der, &SignOptions::new(NOW))
            .expect("prepare");
        let signature = ca.signer().sign(prepared.to_sign()).expect("sign");
        let mut other = unsaved_document();
        other.pages.push(page("another"));
        c.load_document(other);
        let Err(error) = c.finish_sign(prepared, &signature) else {
            panic!("a signature made for another document landed");
        };
        assert!(matches!(error, SignError::DocumentChanged), "{error}");
        assert!(c.document().signatures.items.is_empty());
        assert!(c.document().package.is_none(), "no package was handed over");
        assert!(!c.is_modified());
        assert_eq!(c.body_revision(), 0);
    }

    #[test]
    fn sign_refusal_names_what_stops_a_signature() {
        let mut c = component_with_package();
        assert!(c.sign_refusal().is_ok());
        // A file another tool signed without an annotation entry.
        let bytes = std::fs::read(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../io/tests/fixtures/sign/SESV4SignDoc.ofd"
        ))
        .unwrap();
        c.load_document(rofd_io::parse_ofd(&bytes).unwrap().document);
        assert!(c.sign_refusal().is_ok(), "no annotation to write");
        let page_id = c.document().pages[0].id.clone();
        c.create_annotation(AnnotationKind::Shape(ShapeKind::Rect), page_id, rect());
        assert!(matches!(
            c.sign_refusal(),
            Err(SignError::AnnotationsUnwritable)
        ));
        assert_eq!(
            verify_document(&c, &VerifyOptions::default()).len(),
            1,
            "the file's own signature"
        );
    }

    /// A package whose Document.xml has no `MaxUnitID`, with an element on
    /// its page that rofd does not model: a signature makes the package
    /// again, and reading that package back warns about the element.
    fn package_made_again() -> Vec<u8> {
        rofd_io::zip_util::write_zip(&[
            (
                "OFD.xml".to_owned(),
                br#"<?xml version="1.0" encoding="UTF-8"?><ofd:OFD xmlns:ofd="http://www.ofdspec.org/2016"><ofd:DocBody><ofd:DocRoot>Doc_0/Document.xml</ofd:DocRoot></ofd:DocBody></ofd:OFD>"#.to_vec(),
            ),
            (
                "Doc_0/Document.xml".to_owned(),
                br#"<?xml version="1.0" encoding="UTF-8"?><ofd:Document xmlns:ofd="http://www.ofdspec.org/2016"><ofd:CommonData><ofd:PageArea><ofd:PhysicalBox>0 0 210 297</ofd:PhysicalBox></ofd:PageArea></ofd:CommonData><ofd:Pages><ofd:Page ID="1" BaseLoc="Pages/Page_0/Content.xml"/></ofd:Pages></ofd:Document>"#.to_vec(),
            ),
            (
                "Doc_0/Pages/Page_0/Content.xml".to_owned(),
                br#"<?xml version="1.0" encoding="UTF-8"?><ofd:Page xmlns:ofd="http://www.ofdspec.org/2016"><ofd:Area><ofd:PhysicalBox>0 0 210 297</ofd:PhysicalBox></ofd:Area><ofd:Content><ofd:Unknown/></ofd:Content></ofd:Page>"#.to_vec(),
            ),
        ])
        .expect("zip")
    }

    #[test]
    fn a_signature_whose_package_was_made_again_tells_the_host_what_it_degrades() {
        let mut c = OfdComponent::new_native(OfdConfig::new(Arc::new(vec![])));
        c.set_clock("t".into(), 1);
        c.load_document(rofd_io::parse_ofd(&package_made_again()).unwrap().document);
        let heard = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::clone(&heard);
        c.on_warning(move |warnings| sink.lock().unwrap().push(warnings.to_vec()));

        c.sign(&mut signer(), &SignOptions::new(NOW)).expect("sign");

        let heard = heard.lock().unwrap();
        assert_eq!(heard.len(), 1, "one batch, when the signature landed");
        assert_eq!(heard[0].len(), 1, "{:?}", heard[0]);
        assert!(
            matches!(heard[0][0], OfdWarning::PreservedRaw { .. }),
            "{:?}",
            heard[0][0]
        );
    }

    #[test]
    fn a_landing_with_nothing_to_warn_about_does_not_call_the_host() {
        // A host that keeps the last batch it was given (a native one does)
        // would read an empty one as "this file has no warnings".
        let mut c = component_with_package();
        let heard = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::clone(&heard);
        c.on_warning(move |warnings| sink.lock().unwrap().push(warnings.to_vec()));

        c.sign(&mut signer(), &SignOptions::new(NOW)).expect("sign");

        let heard = heard.lock().unwrap();
        assert!(heard.is_empty(), "no batch without a warning: {heard:?}");
    }

    #[test]
    fn verify_signatures_reads_the_open_document() {
        let mut c = component_with_package();
        assert!(c.verify_signatures(&VerifyOptions::default()).is_empty());
        c.sign(&mut signer(), &seal_options(2)).expect("sign");
        let reports = c.verify_signatures(&VerifyOptions::default());
        assert_eq!(reports.len(), 1);
        assert_eq!(reports[0].overall, SignatureOverall::Valid);
        assert_eq!(reports, verify_document(&c, &VerifyOptions::default()));
    }

    #[test]
    fn sign_pfx_opens_the_certificate_file() {
        // ofdrw's throwaway test key file; the password is 777777.
        let pfx = std::fs::read(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../sign/tests/fixtures/USER.p12"
        ))
        .unwrap();
        let mut c = component_with_package();
        c.clear_modified();
        let error = c
            .sign_pfx(&pfx, "wrong", &SignOptions::new(NOW))
            .expect_err("a wrong password");
        assert!(matches!(error, SignError::Password { .. }), "{error}");
        assert!(!c.is_modified(), "nothing happened");
        assert!(c.verify_signatures(&VerifyOptions::default()).is_empty());

        let reports = c
            .sign_pfx(&pfx, "777777", &SignOptions::new(NOW))
            .expect("sign");
        assert_eq!(reports.len(), 1);
        assert!(c.is_modified());
    }

    #[test]
    fn seal_pictures_follow_the_signature_list() {
        let mut c = component_with_package();
        assert!(c.seal_pictures().is_empty(), "no signature, no picture");
        c.sign(&mut signer(), &seal_options(2)).expect("sign");
        let pictures = c.seal_pictures();
        assert_eq!(pictures.len(), 1);
        let picture = pictures[0].as_ref().expect("a seal carries a picture");
        // The picture the model holds, not a copy of it.
        let held = &c.document().signatures.items[0]
            .seal_picture
            .as_ref()
            .expect("the model holds it")
            .png;
        assert!(Arc::ptr_eq(picture, held));
    }

    fn verify_document(c: &OfdComponent, opts: &VerifyOptions) -> Vec<rofd_sign::SignatureReport> {
        rofd_sign::verify_document(c.document(), opts)
    }
}
