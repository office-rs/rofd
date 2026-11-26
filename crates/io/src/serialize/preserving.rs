//! Preserving save (spec 2026-11-23 §7).
//!
//! The editor save keeps every entry of the original package byte-identical
//! except the annotation entries, which are rewritten in place from the
//! model under their original names. A signed document can therefore gain
//! annotations without its signatures going invalid: the files a signature
//! protects never change.
//!
//! Document.xml is the one exception, and only for unsigned packages: the
//! `MaxUnitID` value is raised when new IDs require it, and the annotation
//! entry is declared when the document has none. A package that declares
//! signatures is never patched.

use std::sync::Arc;

use rofd_dom::{AnnotationModel, OfdDocument, PageId, TextLayoutHints};

use crate::error::OfdError;
use crate::package::PackageHandle;
use crate::serialize::annotation::{preserving_id_seed, serialize_page_annot};
use crate::serialize::annotation_entry::serialize_annotations_entry_with;
use crate::serialize::package::write_ofd_with_text_layout;
use crate::serialize::patch::{max_unit_id_of, with_annotations_loc, with_max_unit_id};
use crate::zip_util::write_zip_borrowed;

/// What happened to Document.xml.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DocumentXmlChange {
    /// There was no package: the file was produced by the full write.
    Generated,
    Untouched,
    /// Only the `MaxUnitID` value changed.
    MaxUnitId,
    /// The annotation entry declaration was inserted (`MaxUnitID` may have
    /// changed as well).
    AnnotationsLocInserted,
}

/// The result of [`save_ofd`].
#[derive(Debug, Clone)]
pub struct Preserved {
    pub bytes: Vec<u8>,
    pub document_xml: DocumentXmlChange,
    /// A required patch had no anchor, so the whole package was written by
    /// the full write instead. Unsigned packages only.
    pub full_write_fallback: bool,
    /// The package declares signatures, has no annotation entry declaration,
    /// and annotations had to be written. Nothing can be written without
    /// touching Document.xml, so `bytes` holds the package unchanged and the
    /// host decides how to proceed.
    pub blocked_by_signature: bool,
    /// The package declares signatures and the IDs of this save exceed the
    /// `MaxUnitID` recorded in Document.xml (the reserve written at signing
    /// time is used up).
    pub reserve_exhausted: bool,
}

/// Why the preserving save could not complete.
#[derive(Debug, thiserror::Error)]
pub enum PreserveError {
    /// An entry file has no place to apply a required patch.
    #[error("cannot patch {entry}: {reason}")]
    PatchAnchorMissing {
        entry: &'static str,
        reason: &'static str,
    },
    #[error(transparent)]
    Io(#[from] OfdError),
}

/// The editor's save entry point: the preserving save over the package the
/// document carries (spec 2026-11-23 §17.11), or the full write when it has
/// none. When an unsigned package offers no anchor for a required
/// Document.xml patch, the full write takes over and the result says so.
pub fn save_ofd(doc: &OfdDocument, text_layout: &TextLayoutHints) -> Result<Preserved, OfdError> {
    let Some(package) = doc.package.as_ref() else {
        return Ok(generated(write_ofd_with_text_layout(
            doc,
            None,
            text_layout,
        )?));
    };
    let page_ids: Vec<PageId> = doc.pages.iter().map(|page| page.id.clone()).collect();
    let id_seed = preserving_id_seed(doc);
    match write_ofd_preserving(package, &page_ids, &doc.annotations, id_seed, text_layout) {
        Ok(preserved) => Ok(preserved),
        Err(PreserveError::Io(error)) => Err(error),
        Err(PreserveError::PatchAnchorMissing { .. }) => {
            // Only unsigned packages are patched, so only they can land here.
            let mut preserved =
                generated(write_ofd_with_text_layout(doc, Some(package), text_layout)?);
            preserved.full_write_fallback = true;
            Ok(preserved)
        }
    }
}

fn generated(bytes: Vec<u8>) -> Preserved {
    Preserved {
        bytes,
        document_xml: DocumentXmlChange::Generated,
        full_write_fallback: false,
        blocked_by_signature: false,
        reserve_exhausted: false,
    }
}

/// The preserving save itself.
///
/// `page_ids` lists the document's page IDs in order and `id_seed` is the
/// highest ID any object may already use (see `preserving_id_seed`). The body
/// is deliberately not an input: this function cannot write body changes
/// (spec I6).
pub fn write_ofd_preserving(
    package: &PackageHandle,
    page_ids: &[PageId],
    annotations: &AnnotationModel,
    id_seed: u64,
    text_layout: &TextLayoutHints,
) -> Result<Preserved, PreserveError> {
    let mut entries: Vec<(String, Arc<Vec<u8>>)> = package
        .entries
        .iter()
        .map(|e| (e.name.clone(), Arc::clone(&e.bytes)))
        .collect();

    let Some(document_entry) = package.document_entry.as_deref() else {
        // Nothing is known about the layout; hand the package back.
        return Ok(Preserved {
            bytes: write_entries(&entries)?,
            document_xml: DocumentXmlChange::Untouched,
            full_write_fallback: false,
            blocked_by_signature: false,
            reserve_exhausted: false,
        });
    };
    let document_dir = parent_dir(document_entry);
    let declared = package.annotations_entry.is_some();
    let manifest_path = package
        .annotations_entry
        .clone()
        .unwrap_or_else(|| join(&document_dir, "Annots/Annotations.xml"));
    let manifest_dir = parent_dir(&manifest_path);

    // The page annotation files to write, in document order. A page that
    // had a file keeps it (an emptied page gets an empty PageAnnot); a page
    // annotated for the first time gets a new file that collides with
    // nothing.
    let mut next_id = id_seed;
    let mut page_files: Vec<(String, String)> = Vec::new();
    let mut manifest_pages: Vec<(PageId, String)> = Vec::new();
    for (index, page_id) in page_ids.iter().enumerate() {
        let page_annotations = annotations.for_page(page_id);
        let existing = package
            .annotation_files
            .iter()
            .find(|(id, _)| id == page_id)
            .map(|(_, path)| path.clone());
        if existing.is_none() && page_annotations.is_empty() {
            continue;
        }
        let path = existing.unwrap_or_else(|| {
            unused_path(
                &entries,
                &join(&manifest_dir, &format!("Page_{index}/Annotation.xml")),
            )
        });
        page_files.push((
            path.clone(),
            serialize_page_annot(page_id, page_annotations, &mut next_id, text_layout),
        ));
        // A page file that is not below the entry file's directory is named
        // through `..`: it stays where it is, and the loc still resolves.
        manifest_pages.push((
            page_id.clone(),
            crate::paths::relative_to(&path, &manifest_dir),
        ));
    }
    // The allocator ends on the highest ID it minted.
    let final_id = next_id;
    let manifest_needed =
        !manifest_pages.is_empty() || position(&entries, &manifest_path).is_some();
    let must_declare = manifest_needed && !declared;

    let document_text = position(&entries, document_entry)
        .and_then(|i| std::str::from_utf8(&entries[i].1).ok().map(str::to_owned));

    let mut change = DocumentXmlChange::Untouched;
    let blocked = annotations_unwritable(package, page_ids, annotations);
    if !package.signatures_declared {
        let document_text = document_text
            .as_deref()
            .ok_or(PreserveError::PatchAnchorMissing {
                entry: "Document.xml",
                reason: "the entry is missing or is not UTF-8 text",
            })?;
        let mut patched = document_text.to_owned();
        if must_declare {
            patched = with_annotations_loc(
                &patched,
                &crate::paths::relative_to(&manifest_path, &document_dir),
            )
            .ok_or(PreserveError::PatchAnchorMissing {
                entry: "Document.xml",
                reason: "no place to declare the annotation entry file",
            })?;
            change = DocumentXmlChange::AnnotationsLocInserted;
        }
        patched =
            with_max_unit_id(&patched, final_id).ok_or(PreserveError::PatchAnchorMissing {
                entry: "Document.xml",
                reason: "MaxUnitID is missing or is not an integer",
            })?;
        if change == DocumentXmlChange::Untouched && patched != document_text {
            change = DocumentXmlChange::MaxUnitId;
        }
        set_entry(&mut entries, document_entry, patched.into_bytes());
    }

    let reserve_exhausted = package.signatures_declared
        && final_id > max_unit_id_of(document_text.as_deref().unwrap_or("")).unwrap_or(0);

    if !blocked {
        if manifest_needed {
            set_entry(
                &mut entries,
                &manifest_path,
                serialize_annotations_entry_with(&manifest_pages).into_bytes(),
            );
        }
        for (path, xml) in page_files {
            set_entry(&mut entries, &path, xml.into_bytes());
        }
    }

    Ok(Preserved {
        bytes: write_entries(&entries)?,
        document_xml: change,
        full_write_fallback: false,
        blocked_by_signature: blocked,
        reserve_exhausted,
    })
}

fn parent_dir(path: &str) -> String {
    path.rsplit_once('/')
        .map_or(String::new(), |(dir, _)| dir.to_owned())
}

/// Whether annotations have to be written into a package that declares
/// signatures and declares no annotation entry (spec 2026-11-23 M16): nothing
/// can be written without changing Document.xml, which a signature protects.
/// The preserving save reports it as [`Preserved::blocked_by_signature`], and
/// a signature refuses such a document before anything is made, through the
/// same decision.
pub fn annotations_unwritable(
    package: &PackageHandle,
    page_ids: &[PageId],
    annotations: &AnnotationModel,
) -> bool {
    if !package.signatures_declared || package.annotations_entry.is_some() {
        return false;
    }
    let Some(document_entry) = package.document_entry.as_deref() else {
        // Nothing is known about the layout; the save hands the package back.
        return false;
    };
    // No entry is declared (above), so this is the place a signature's
    // scaffold would put one.
    let manifest_path = join(&parent_dir(document_entry), "Annots/Annotations.xml");
    // A page with annotations to write, or with an annotation file to
    // rewrite; or an entry file the package carries without a declaration.
    let pages = page_ids.iter().any(|page_id| {
        !annotations.for_page(page_id).is_empty()
            || package.annotation_files.iter().any(|(id, _)| id == page_id)
    });
    pages || package.find(&manifest_path).is_some()
}

fn join(dir: &str, relative: &str) -> String {
    if dir.is_empty() {
        relative.to_owned()
    } else {
        format!("{dir}/{relative}")
    }
}

fn position(entries: &[(String, Arc<Vec<u8>>)], name: &str) -> Option<usize> {
    entries.iter().position(|(entry, _)| entry == name)
}

/// Replace an entry in place, keeping its position, or append it.
fn set_entry(entries: &mut Vec<(String, Arc<Vec<u8>>)>, name: &str, bytes: Vec<u8>) {
    match position(entries, name) {
        Some(i) => entries[i].1 = Arc::new(bytes),
        None => entries.push((name.to_owned(), Arc::new(bytes))),
    }
}

/// `base`, or `base` with a suffix appended when an entry of that name
/// already exists and is not one of ours.
fn unused_path(entries: &[(String, Arc<Vec<u8>>)], base: &str) -> String {
    let mut candidate = base.to_owned();
    let mut attempt = 1;
    while position(entries, &candidate).is_some() {
        candidate = if attempt == 1 {
            format!("{base}_rofd")
        } else {
            format!("{base}_rofd{attempt}")
        };
        attempt += 1;
    }
    candidate
}

fn write_entries(entries: &[(String, Arc<Vec<u8>>)]) -> Result<Vec<u8>, OfdError> {
    let borrowed: Vec<(&str, &[u8])> = entries
        .iter()
        .map(|(name, bytes)| (name.as_str(), bytes.as_slice()))
        .collect();
    write_zip_borrowed(&borrowed)
}
