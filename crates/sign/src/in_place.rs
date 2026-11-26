//! Signing in place (spec 2026-11-23 §17.11): the signature goes into the
//! document that is open.
//!
//! A signature is made over the bytes of the package the document carries,
//! and it is appended to that package. No model is parsed again, no package
//! is made again, and no signed copy of the file exists before the user
//! saves. What a signature writes is small and fixed: its own entries, and
//! on the first signature of a package the patches to OFD.xml and
//! Document.xml that a signed package could no longer take. Everything is
//! computed first and written in one step, so a refusal or a failure leaves
//! the document as it was.
//!
//! Annotations are no part of a signature (they are not protected, spec
//! I4), so they are neither read nor written here: they stay what the
//! editor holds, and the save writes them as it always does.
//!
//! With a seal picture the signer signs twice (the seal being made, then the
//! signature value); without a picture once, a GB/T 35275 signed message.
//! [`prepare_in_place`] and [`PreparedInPlace`] expose the rounds one by
//! one, for a signer that is not in this process; [`sign_in_place`] runs
//! them in one call.

use std::sync::Arc;

use rofd_dom::{OfdDocument, OfdWarning, PageId, TextLayoutHints};
use rofd_io::{
    annotations_unwritable, generate_package, refresh_document_level, EntryKind, GeneratedPackage,
    OfdError, PackageHandle,
};

use crate::crypto::{sm2_verify, sm3, DEFAULT_SM2_USER_ID};
use crate::error::SignError;
use crate::gbt35275::build_signed_data;
use crate::package_util::{
    annotation_layer, join, scaffolds, set_entry, signature_list_xml, under, Layout, Patch,
    Scaffold,
};
use crate::report::{SignatureReport, VerifyOptions};
use crate::ses_encoder::{build_e_seal_info, build_tbs, finish_seal, finish_signature, SealInput};
use crate::sign::{invalid, plan, Plan, SignOptions};
use crate::signer::Signer;
use crate::verify::verify_package;

/// What a signature does when the package the document carries cannot take
/// the patch the first signature has to make to Document.xml.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Regenerate {
    /// A document that is open gets a generated package first, once - the
    /// same thing a save of such a file falls back to. Nothing protects the
    /// file, or it would not be patched.
    Once,
    /// Bytes that were handed in are refused, as `sign` always did.
    Never,
}

/// The document as it was when a signature was planned: what the signature
/// is checked against when it lands.
///
/// A signature is made over the entries of one package, so those are what
/// is recognised - every entry it covers, and the signature list, by name
/// and bytes. Three files are not enough: two packages can share OFD.xml,
/// Document.xml and the list and differ in a page, and a signature made for
/// one of them reads `Invalid` in the other. The buffers are held, so the
/// document's own are recognised by one pointer comparison each, and the
/// same file read again is compared byte for byte and passes. What a
/// signature leaves out is left out here, by the same list: the annotation
/// layer as the package locates it, and directory entries. A file that
/// merely looks like an annotation and is not located is compared - a
/// signature protects it.
///
/// A document without a package has no bytes to be recognised by, and the
/// package made for its signature is not handed over before the signature
/// lands. What a package is generated from stands in: the model without the
/// annotations.
pub(crate) struct Seen {
    /// `(name, bytes)` of every entry but the annotation layer and the
    /// directory entries, in entry order.
    entries: Vec<(String, Arc<Vec<u8>>)>,
    /// SM3 of the model a package would be generated from. `Some` only for
    /// a document without a package.
    bare: Option<[u8; 32]>,
}

impl Seen {
    fn of(document: &OfdDocument) -> Self {
        let Some(package) = document.package.as_ref() else {
            return Self {
                entries: Vec::new(),
                bare: Some(bare_fingerprint(document)),
            };
        };
        let left_out = annotation_layer(package);
        Self {
            entries: package
                .entries
                .iter()
                .filter(|entry| !entry.name.ends_with('/') && !left_out.contains(&entry.name))
                .map(|entry| (entry.name.clone(), Arc::clone(&entry.bytes)))
                .collect(),
            bare: None,
        }
    }
}

/// Whether the document is the one that was seen: the same entries in the
/// same order, each in the same buffer or with the same bytes; for a
/// document without a package, the same model.
impl PartialEq for Seen {
    fn eq(&self, other: &Self) -> bool {
        self.bare == other.bare
            && self.entries.len() == other.entries.len()
            && self
                .entries
                .iter()
                .zip(&other.entries)
                .all(|((seen_name, seen), (name, now))| {
                    seen_name == name && (Arc::ptr_eq(seen, now) || seen == now)
                })
    }
}

// Not derived: it would print every protected byte of the package.
impl std::fmt::Debug for Seen {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Seen")
            .field("entries", &self.entries.len())
            .field("bare", &self.bare.is_some())
            .finish()
    }
}

/// What a package is generated from, for a document without one: the model
/// without the annotations, which are no part of a signature - an edit of
/// them between preparing and landing is no change. `max_unit_id` goes with
/// them: the editor raises it as it numbers an annotation on an unsigned
/// document, and the first signature's scaffold writes the ID reserve over
/// it, above every ID an edit in that time can take. A signature that landed
/// in the meantime is not asked about here: it hands the document a package.
fn bare_fingerprint(document: &OfdDocument) -> [u8; 32] {
    let mut bare = document.clone();
    bare.annotations = Default::default();
    bare.max_unit_id = 0;
    bare.signatures = Default::default();
    bare.body_revision = 0;
    sm3(&serde_json::to_vec(&bare).expect("a document serializes"))
}

/// What stops a signature of `document` right now, before anything is made
/// (spec §17.11): the document has annotations, and its file declares
/// signatures and declares no annotation entry, so they cannot be written
/// into it. The same decision the preserving save makes when it reports
/// `blocked_by_signature`.
pub fn sign_refusal(document: &OfdDocument) -> Result<(), SignError> {
    let Some(package) = document.package.as_ref() else {
        return Ok(());
    };
    let page_ids: Vec<PageId> = document.pages.iter().map(|page| page.id.clone()).collect();
    if annotations_unwritable(package, &page_ids, &document.annotations) {
        return Err(SignError::AnnotationsUnwritable);
    }
    Ok(())
}

/// Plan a signature on the package `document` is saved with, in a fixed
/// order:
///
/// 1. what can be refused is refused, with nothing changed ([`sign_refusal`]);
/// 2. a document without a package gets one made, when `regenerate` allows
///    it - the only place a package is made for a signature;
/// 3. a package whose first signature could not patch Document.xml gets a
///    generated one in its place, again only when allowed: the full write
///    produces an anchor, and what the model does not carry passes through;
/// 4. the plan is made, and it remembers the document's own package as it
///    was then.
pub(crate) fn plan_for(
    document: &OfdDocument,
    certificate_der: &[u8],
    opts: &SignOptions,
    regenerate: Regenerate,
) -> Result<Plan, SignError> {
    sign_refusal(document)?;
    let may_generate = matches!(regenerate, Regenerate::Once);
    let made = match document.package.as_ref() {
        None if may_generate => Some(make_a_package(document)?),
        Some(own) if may_generate && first_signature_has_no_anchor(own, document) => {
            Some(make_a_package(document)?)
        }
        _ => None,
    };
    let package = made
        .as_ref()
        .map(|made| &made.package)
        .or(document.package.as_ref())
        .ok_or_else(|| invalid("the document has no package to sign"))?;
    // The document as it is now: the signature is checked against this
    // record when it lands.
    let mut plan = plan(package, document, certificate_der, opts, Seen::of(document))?;
    if let Some(GeneratedPackage { package, warnings }) = made {
        plan.package = Some(package);
        plan.warnings = warnings;
    }
    Ok(plan)
}

/// The package the full write makes for `document`: what a signature lands
/// on when the document has none, or none that can take the first
/// signature's patch. The text layout is not consulted - annotations are no
/// part of a signature, and the next save rewrites them with the host's.
fn make_a_package(document: &OfdDocument) -> Result<rofd_io::GeneratedPackage, SignError> {
    Ok(generate_package(document, &TextLayoutHints::new())?)
}

/// Whether the first signature of `package` would have to patch Document.xml
/// where there is nothing to patch at. Only a package without a signature is
/// patched, so nothing protects the file.
fn first_signature_has_no_anchor(package: &PackageHandle, document: &OfdDocument) -> bool {
    let Ok(layout) = Layout::of(package) else {
        return false;
    };
    layout.list.records.is_empty()
        && matches!(
            scaffolds(package, document, &layout),
            Ok(Scaffold::NoAnchor(_))
        )
}

// ---- the rounds ----

/// A signature prepared on a document and not yet in it: everything is
/// computed up to the round the signer has to produce.
///
/// [`Self::to_sign`] is what the signer signs; [`Self::apply`] takes that
/// signature and either yields the next round or the finished signature,
/// which still has to be put into the document with
/// [`SignatureToAppend::append_to`]. Preparing changes nothing.
#[derive(Debug)]
pub struct PreparedInPlace {
    stage: Stage,
}

/// The plan is boxed: a [`PreparedInPlace`] then weighs about as much as a
/// [`SignatureToAppend`], and it is passed around by value.
#[derive(Debug)]
enum Stage {
    /// A seal is being made; `e_seal_info` is to be signed.
    Seal {
        plan: Box<Plan>,
        e_seal_info: Vec<u8>,
    },
    /// The signature value is being made; `to_sign` is to be signed.
    Value {
        plan: Box<Plan>,
        /// The finished seal (`Seal.esl`), when there is one.
        seal: Option<Vec<u8>>,
        description: Vec<u8>,
        to_sign: Vec<u8>,
    },
}

impl PreparedInPlace {
    /// The first round of a signature planned on a document: the seal being
    /// made when there is a picture, otherwise the signature value at once.
    pub(crate) fn first(plan: Plan) -> Result<Self, SignError> {
        let certificate = plan.certificate.clone();
        let plan = Box::new(plan);
        let Some(picture) = plan.picture.clone() else {
            return plan.value_stage(None);
        };
        let e_seal_info = build_e_seal_info(
            &SealInput {
                png: &picture.png,
                width: picture.width_mm,
                height: picture.height_mm,
                certificate_der: &certificate,
            },
            &plan.now,
        )?;
        Ok(Self {
            stage: Stage::Seal { plan, e_seal_info },
        })
    }

    /// What the signer signs in this round.
    pub fn to_sign(&self) -> &[u8] {
        match &self.stage {
            Stage::Seal { e_seal_info, .. } => e_seal_info,
            Stage::Value { to_sign, .. } => to_sign,
        }
    }

    /// Take the signature of [`Self::to_sign`] and go on: the round that
    /// produces the signature value, or the finished signature. A signature
    /// that does not verify with the signer's certificate is not kept: the
    /// document is not changed either way.
    pub fn apply(self, signature: &[u8]) -> Result<InPlaceStep, SignError> {
        let Self { stage } = self;
        match stage {
            Stage::Seal { plan, e_seal_info } => {
                confirm_signature(&plan.public_key, &e_seal_info, signature)?;
                let seal = finish_seal(&e_seal_info, signature, &plan.certificate)?;
                Ok(InPlaceStep::More(plan.value_stage(Some(seal))?))
            }
            Stage::Value {
                plan,
                seal,
                description,
                to_sign,
            } => {
                confirm_signature(&plan.public_key, &to_sign, signature)?;
                let value = match &seal {
                    Some(_) => finish_signature(&to_sign, signature, &plan.certificate)?,
                    None => build_signed_data(&to_sign, signature, &plan.certificate)?,
                };
                Ok(InPlaceStep::Done(Box::new(SignatureToAppend {
                    plan: *plan,
                    seal,
                    description,
                    value,
                })))
            }
        }
    }
}

impl Plan {
    /// The round that produces the signature value: the to-be-signed data
    /// with the seal, or the digest of the description file without one.
    fn value_stage(self: Box<Self>, seal: Option<Vec<u8>>) -> Result<PreparedInPlace, SignError> {
        let description = self.description(seal.as_deref()).into_bytes();
        let data_hash = sm3(&description);
        let to_sign = match &seal {
            Some(seal) => build_tbs(seal, &data_hash, &self.file_loc("Signature.xml"), &self.now)?,
            None => data_hash.to_vec(),
        };
        Ok(PreparedInPlace {
            stage: Stage::Value {
                plan: self,
                seal,
                description,
                to_sign,
            },
        })
    }
}

/// What comes out of [`PreparedInPlace::apply`]: the next round to sign, or
/// the finished signature, not in the document yet.
#[derive(Debug)]
pub enum InPlaceStep {
    More(PreparedInPlace),
    Done(Box<SignatureToAppend>),
}

/// A finished signature that is not in the document yet;
/// [`Self::append_to`] puts it there.
#[derive(Debug)]
pub struct SignatureToAppend {
    plan: Plan,
    seal: Option<Vec<u8>>,
    description: Vec<u8>,
    value: Vec<u8>,
}

/// What a signature leaves behind when it lands in a document.
#[derive(Debug)]
pub struct Landed {
    /// The verdict on every signature of the document afterwards.
    pub reports: Vec<SignatureReport>,
    /// What the landing had to tell the host: the warnings of reading back a
    /// package that was made for the signature, and those of the reread that
    /// are about the signature that landed. The reread projects every
    /// signature of the list again, and a warning about one that was in the
    /// file already was reported when the file was opened; it is not
    /// repeated here.
    pub warnings: Vec<OfdWarning>,
}

impl SignatureToAppend {
    /// Put the signature into `document`, in one step: the scaffolds when it
    /// is the first signature of the package, then the signature's own
    /// entries; the package the document was given for it, when it had none.
    /// The document's document-level fields are read again from its package,
    /// so they are what loading the package would give; the pages, the
    /// resources and the annotations are not touched. Returns the reports of
    /// every signature of the document afterwards, and the warnings of the
    /// landing ([`Landed`]).
    ///
    /// The package is written beside the document, takes the document's
    /// place, and the document is read back from it; when that reading
    /// fails, the document's own package goes back - the document is as it
    /// was. That holds because the reading does everything that can fail
    /// before it writes a field (`rofd_io::refresh_document_level` says so
    /// where it matters).
    ///
    /// The signature was prepared on this document's package and lands on no
    /// other: when another signature landed in the meantime, or a save
    /// patched the file, it is refused with [`SignError::DocumentChanged`] and
    /// nothing is written. An edit of the annotations in the meantime is no
    /// change - annotations are no part of a signature.
    pub fn append_to(self, document: &mut OfdDocument) -> Result<Landed, SignError> {
        self.land(document, true)
    }

    /// [`Self::append_to`] without the verification for the bytes API, which
    /// returns a file and not reports.
    pub(crate) fn land(
        self,
        document: &mut OfdDocument,
        verify: bool,
    ) -> Result<Landed, SignError> {
        self.land_with(document, verify, refresh_document_level)
    }

    /// [`Self::land`] with the reread of the document handed in, so that a
    /// test can make it fail.
    fn land_with(
        self,
        document: &mut OfdDocument,
        verify: bool,
        refresh: Refresh,
    ) -> Result<Landed, SignError> {
        let Self {
            plan,
            seal,
            description,
            value,
        } = self;
        let mut plan = plan;
        // Prepared on this document and on no other. What the plan saw is
        // compared with what the document holds now: a signature that landed
        // in the meantime, or another document - the same file saved and
        // opened again included - shows in it. An edit of the annotations
        // does not: no signature covers them, and they are not in the
        // package until a save writes them.
        if plan.seen != Seen::of(document) {
            return Err(SignError::DocumentChanged);
        }
        // The package as it will be, written beside the document: the one
        // made for the signature, or a copy of the document's own handle -
        // its entry buffers are shared, not copied. Nothing of the document
        // changes until the new package has been read back.
        let mut written = match plan.package.take() {
            Some(made) => made,
            None => document
                .package
                .as_ref()
                .cloned()
                .ok_or_else(|| invalid("the document has no package to sign"))?,
        };
        if let Some(scaffolds) = plan.scaffolds.take() {
            for patch in scaffolds.patches {
                set_entry(&mut written, patch);
            }
            if let Some(entry) = scaffolds.annotations_entry {
                written.annotations_entry = Some(entry);
            }
        }
        let entry = |name: String, bytes: Vec<u8>| Patch {
            kind: EntryKind::of(&name),
            name,
            bytes,
            // Signature entries are only ever read, never written by a
            // save: the full write carries them through.
            consumed: false,
        };
        if let Some(seal) = seal {
            set_entry(&mut written, entry(join(&plan.dir, "Seal.esl"), seal));
        }
        set_entry(
            &mut written,
            entry(join(&plan.dir, "SignedValue.dat"), value),
        );
        set_entry(
            &mut written,
            entry(join(&plan.dir, "Signature.xml"), description),
        );
        let list = signature_list_xml(&plan.list, &plan.record, &plan.max_sign_id);
        set_entry(
            &mut written,
            entry(plan.list_path.clone(), list.into_bytes()),
        );
        // The new package takes the document's place; what the model and the
        // handle keep of OFD.xml, Document.xml and the signature entries is
        // read again from it. The document's own package stays at hand until
        // that reading has succeeded: if it fails, it goes back.
        let own = document.package.replace(written);
        let reread = match refresh(document) {
            Ok(warnings) => warnings,
            Err(error) => {
                document.package = own;
                return Err(error.into());
            }
        };
        // What reading the package made for the signature warned about, and
        // what the reread says about the signature that landed. The rest of
        // the reread is about signatures that were in the file already, and
        // was reported when the file was opened.
        let mut warnings = std::mem::take(&mut plan.warnings);
        warnings.extend(
            reread
                .into_iter()
                .filter(|warning| plan.is_about_the_signature(warning)),
        );
        let reports = if verify {
            verify_package(
                document
                    .package
                    .as_ref()
                    .expect("the package was put there above"),
                &VerifyOptions::default(),
            )
        } else {
            Vec::new()
        };
        Ok(Landed { reports, warnings })
    }
}

impl Plan {
    /// Whether a warning of the reread is about the signature that was
    /// planned here: the list it rewrote, or a file under its directory. The
    /// reread projects every signature of the list; about the others it says
    /// what reading the file said when it was opened.
    fn is_about_the_signature(&self, warning: &OfdWarning) -> bool {
        let entry = match warning {
            OfdWarning::MissingFeature { entry, .. } | OfdWarning::PreservedRaw { entry, .. } => {
                entry
            }
            // Nothing to tell it by: it is kept, not dropped for lack of a name.
            _ => return true,
        };
        entry == &self.list_path || under(entry, &self.dir)
    }
}

/// How the document is read again once the signature is in its package.
/// [`refresh_document_level`] is the one; the tests of the landing hand in
/// their own, to see it fail.
type Refresh = fn(&mut OfdDocument) -> Result<Vec<OfdWarning>, OfdError>;

type RoundSigner<'a> = &'a mut dyn FnMut(&[u8]) -> Result<Vec<u8>, SignError>;

fn rounds(
    mut prepared: PreparedInPlace,
    signer: RoundSigner<'_>,
) -> Result<Box<SignatureToAppend>, SignError> {
    loop {
        let signature = signer(prepared.to_sign())?;
        match prepared.apply(&signature)? {
            InPlaceStep::More(next) => prepared = next,
            InPlaceStep::Done(appendix) => return Ok(appendix),
        }
    }
}

/// Confirm that `signature` is an SM2 signature of `message` by the key of
/// `public_key`. A signer that signed something else would otherwise only
/// be noticed by whoever verifies the document later.
fn confirm_signature(public_key: &[u8], message: &[u8], signature: &[u8]) -> Result<(), SignError> {
    if signature.is_empty() {
        return Err(SignError::Signer {
            reason: "the signature is empty".to_owned(),
        });
    }
    if !sm2_verify(public_key, DEFAULT_SM2_USER_ID, message, signature) {
        return Err(SignError::Signer {
            reason: "the signature does not verify with the signer's certificate".to_owned(),
        });
    }
    Ok(())
}

// ---- what a host calls ----

/// Append a signature to `document`: the document that is open, the one the
/// user is looking at (spec 2026-11-23 §17.11).
///
/// The signature goes into the package the document carries, and the
/// document's signature list names it afterwards. Nothing else of the
/// document changes - not the pages, not the annotations, not a view, a
/// selection or an undo history a host keeps on it - and nothing is
/// written to a file: that is the host's save, which is an ordinary save.
///
/// Refused, with nothing changed: a document whose file cannot take its
/// annotations ([`sign_refusal`]), an option that is unusable (a picture
/// that does not decode, a position the seal does not fit into). A document
/// without a package is given one, once. Returns what the landing leaves:
/// the reports of every signature of the document afterwards, and the
/// warnings the host is to hear ([`Landed`]).
pub fn sign_in_place(
    document: &mut OfdDocument,
    signer: &mut dyn Signer,
    opts: &SignOptions,
) -> Result<Landed, SignError> {
    let plan = plan_for(document, signer.certificate_der(), opts, Regenerate::Once)?;
    let appendix = rounds(PreparedInPlace::first(plan)?, &mut |to_sign| {
        signer.sign(to_sign)
    })?;
    appendix.land(document, true)
}

/// The bytes API behind `sign` and `prepare_sign`: a document parsed from
/// bytes that were handed in. No package is generated for a document that
/// cannot take the first signature's patch - refused, as it always was -
/// and the document is not verified afterwards: the caller gets a file, not
/// reports. Nothing is lost by taking neither the reports nor the warnings
/// of the landing ([`Landed`]): no package is made here, and what the reread
/// warns about is kept only when it is about the signature that landed - a
/// signature rofd-sign wrote, which reads back.
pub(crate) fn sign_bytes(
    document: &mut OfdDocument,
    signer: &mut dyn Signer,
    opts: &SignOptions,
) -> Result<(), SignError> {
    let plan = plan_for(document, signer.certificate_der(), opts, Regenerate::Never)?;
    let appendix = rounds(PreparedInPlace::first(plan)?, &mut |to_sign| {
        signer.sign(to_sign)
    })?;
    appendix.land(document, false)?;
    Ok(())
}

/// Do everything up to the first signature a signer has to produce, for the
/// document that is open: the two-phase form of [`sign_in_place`], for a
/// signer that is not in this process.
///
/// `certificate_der` is the signer's certificate: it is embedded in the seal
/// and in the signature value, and every signature handed to
/// [`PreparedInPlace::apply`] must verify against it. Preparing changes
/// nothing of the document.
pub fn prepare_in_place(
    document: &OfdDocument,
    certificate_der: &[u8],
    opts: &SignOptions,
) -> Result<PreparedInPlace, SignError> {
    PreparedInPlace::first(plan_for(document, certificate_der, opts, Regenerate::Once)?)
}

/// Verify every signature of the package `document` carries, without
/// making a file of it first. A document without a package has none.
pub fn verify_document(document: &OfdDocument, opts: &VerifyOptions) -> Vec<SignatureReport> {
    document
        .package
        .as_ref()
        .map_or_else(Vec::new, |package| verify_package(package, opts))
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::mock_ca::TestCa;

    const NOW: &str = "2026-11-24 10:30:00";

    /// A document with a package, the way a host opens one: one empty page,
    /// written by the full write and read back.
    fn document() -> OfdDocument {
        let mut document = OfdDocument::default();
        document.pages.push(rofd_dom::Page {
            raw_children: Vec::new(),
            id: PageId::new("1"),
            physical_box: rofd_dom::Rect {
                x: 0.0,
                y: 0.0,
                w: 210.0,
                h: 297.0,
            },
            layers: Vec::new(),
            template: None,
        });
        let bytes = rofd_io::write_ofd(&document).expect("write");
        rofd_io::parse_ofd(&bytes).expect("parse").document
    }

    /// A finished digital signature for `document`, not in it yet.
    fn appendix(document: &OfdDocument) -> SignatureToAppend {
        let mut signer = TestCa::from_seed("rofd test signer", b"seed-a").signer();
        let plan = plan_for(
            document,
            signer.certificate_der(),
            &SignOptions::new(NOW),
            Regenerate::Once,
        )
        .expect("plan");
        let mut signing = |to_sign: &[u8]| signer.sign(to_sign);
        *rounds(
            PreparedInPlace::first(plan).expect("first round"),
            &mut signing,
        )
        .expect("rounds")
    }

    #[test]
    fn a_failed_reread_leaves_the_document_as_it_was() {
        let mut document = document();
        let before: Vec<Arc<Vec<u8>>> = document
            .package
            .as_ref()
            .expect("parsed")
            .entries
            .iter()
            .map(|entry| Arc::clone(&entry.bytes))
            .collect();
        let signatures = document.signatures.clone();
        let failing: Refresh = |_| {
            Err(OfdError::Schema {
                entry: "OFD.xml".into(),
                reason: "made to fail".into(),
            })
        };
        let error = appendix(&document)
            .land_with(&mut document, true, failing)
            .expect_err("the reread failed");
        assert!(matches!(error, SignError::Io(_)), "{error}");
        let package = document.package.as_ref().expect("parsed");
        assert_eq!(package.entries.len(), before.len(), "nothing was written");
        assert!(
            package
                .entries
                .iter()
                .zip(&before)
                .all(|(entry, bytes)| Arc::ptr_eq(&entry.bytes, bytes)),
            "the same buffers as before"
        );
        assert!(!package.signatures_declared);
        assert_eq!(document.signatures, signatures);
    }

    #[test]
    fn only_the_warnings_about_the_signature_that_landed_come_back() {
        let mut document = document();
        // A first signature, so that the one under test gets its own
        // directory.
        let mut signer = TestCa::from_seed("rofd test signer", b"seed-a").signer();
        sign_in_place(&mut document, &mut signer, &SignOptions::new(NOW)).expect("first");
        assert_eq!(document.signatures.items.len(), 1);
        let appendix = appendix(&document);
        let landed = appendix
            .land_with(&mut document, false, |document| {
                let mut warnings = refresh_document_level(document)?;
                warnings.push(OfdWarning::MissingFeature {
                    feature: "made up: the signature that landed".into(),
                    entry: "Doc_0/Signs/Sign_1/Signature.xml".into(),
                });
                warnings.push(OfdWarning::MissingFeature {
                    feature: "made up: a signature that was already there".into(),
                    entry: "Doc_0/Signs/Sign_0/Signature.xml".into(),
                });
                warnings.push(OfdWarning::SkippedObject {
                    page: PageId::new("1"),
                    reason: "made up: nothing to tell it by".into(),
                });
                Ok(warnings)
            })
            .expect("land");
        // The reread warns about every signature of the list again. What was
        // reported when the file was opened is not repeated; what names no
        // entry cannot be attributed, and is kept.
        assert_eq!(
            landed.warnings,
            vec![
                OfdWarning::MissingFeature {
                    feature: "made up: the signature that landed".into(),
                    entry: "Doc_0/Signs/Sign_1/Signature.xml".into(),
                },
                OfdWarning::SkippedObject {
                    page: PageId::new("1"),
                    reason: "made up: nothing to tell it by".into(),
                },
            ]
        );
    }
}
