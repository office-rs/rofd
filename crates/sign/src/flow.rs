//! What every host does the same way around signing and verification, with
//! no UI in it (spec 2026-11-23 §10, §13).

use crate::report::{SignatureOverall, SignatureReport};
use crate::sign::{Pages, Placement, DEFAULT_MARGIN_MM};

/// What the signing dialog collected.
#[derive(Debug, Clone, PartialEq)]
pub struct SignRequest {
    /// An electronic signature: no picture, so no placement state either.
    pub no_picture: bool,
    /// Auto-stamping: the seal goes to the given position, no click needed.
    pub auto_place: bool,
    /// Distance from the right edge of the page, millimetres.
    pub right_mm: f64,
    /// Distance from the bottom edge of the page, millimetres.
    pub bottom_mm: f64,
    /// The last page only; otherwise every page.
    pub last_page_only: bool,
}

impl Default for SignRequest {
    fn default() -> Self {
        Self::new()
    }
}

impl SignRequest {
    /// The defaults of the dialog: a seal placed by hand.
    pub fn new() -> Self {
        Self {
            no_picture: false,
            auto_place: false,
            right_mm: DEFAULT_MARGIN_MM,
            bottom_mm: DEFAULT_MARGIN_MM,
            last_page_only: true,
        }
    }

    /// Whether the document can be signed right away, without the placement
    /// state: there is no picture, or the position is given.
    pub fn signs_directly(&self) -> bool {
        self.no_picture || self.auto_place
    }

    /// The position of an auto-stamped seal. `None` when the user is to
    /// place it with a click.
    pub fn placement(&self) -> Option<Placement> {
        self.auto_place.then_some(Placement::BottomRight {
            pages: if self.last_page_only {
                Pages::Last
            } else {
                Pages::All
            },
            right_mm: self.right_mm,
            bottom_mm: self.bottom_mm,
        })
    }
}

/// What the status bar says about the signatures of a document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SignatureSummary {
    /// The document has no signature.
    None,
    /// Every signature is valid.
    AllValid(usize),
    /// At least one signature failed: how many, of how many.
    SomeInvalid { invalid: usize, total: usize },
    /// Nothing failed, but `n` signatures could not be confirmed for lack
    /// of material. Any others are valid.
    OnlyUnverified(usize),
}

/// Summarize the verification reports of a document. A failure outranks
/// everything else.
pub fn summarize(reports: &[SignatureReport]) -> SignatureSummary {
    if reports.is_empty() {
        return SignatureSummary::None;
    }
    let invalid = reports
        .iter()
        .filter(|report| report.overall == SignatureOverall::Invalid)
        .count();
    if invalid > 0 {
        return SignatureSummary::SomeInvalid {
            invalid,
            total: reports.len(),
        };
    }
    let valid = reports
        .iter()
        .filter(|report| report.overall == SignatureOverall::Valid)
        .count();
    if valid == reports.len() {
        SignatureSummary::AllValid(valid)
    } else {
        SignatureSummary::OnlyUnverified(reports.len() - valid)
    }
}

/// The signatures that were valid in `before` and are no longer in `after`:
/// their IDs, in the order of `before`. A signature `after` does not contain
/// counts as no longer valid.
///
/// A report is matched to its earlier one by ID; an ID that `after` does not
/// carry is matched by position, for lists whose IDs are not unique.
pub fn newly_invalidated(before: &[SignatureReport], after: &[SignatureReport]) -> Vec<String> {
    before
        .iter()
        .enumerate()
        .filter(|(index, report)| {
            let later = after
                .iter()
                .find(|later| later.id == report.id)
                .or_else(|| after.get(*index));
            report.overall == SignatureOverall::Valid
                && later.is_none_or(|later| later.overall != SignatureOverall::Valid)
        })
        .map(|(_, report)| report.id.clone())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::report::{
        IntegrityResult, SignatureOverall, SignatureValue, SignatureValueStatus, SignatureVariant,
    };
    use rofd_dom::SignatureKind;

    fn report(id: &str, overall: SignatureOverall) -> SignatureReport {
        SignatureReport {
            id: id.to_owned(),
            kind: SignatureKind::Seal,
            variant: SignatureVariant::SesV4,
            overall,
            integrity: IntegrityResult {
                ok: true,
                failures: Vec::new(),
                protected_files: 1,
            },
            seal_match: None,
            signature_value: SignatureValue {
                status: SignatureValueStatus::Ok,
                reason: None,
            },
            certificate: None,
            provider: None,
            date_time: None,
        }
    }

    fn auto_place(request: &mut SignRequest) -> &mut SignRequest {
        request.auto_place = true;
        request
    }

    #[test]
    fn request_without_picture_or_with_auto_place_signs_directly() {
        // A seal the user places with a click: the host enters the placement state.
        let request = SignRequest::new();
        assert!(!request.signs_directly());
        assert_eq!(request.placement(), None);
        assert_eq!(
            (
                request.no_picture,
                request.auto_place,
                request.right_mm,
                request.bottom_mm,
                request.last_page_only
            ),
            (false, false, 10.0, 10.0, true),
            "the defaults of the dialog"
        );
        // An electronic signature has no picture to place.
        let mut request = SignRequest::new();
        request.no_picture = true;
        assert!(request.signs_directly());
        assert_eq!(request.placement(), None, "nothing is placed");
        // Auto-stamping needs no click either.
        let mut request = SignRequest::new();
        assert!(auto_place(&mut request).signs_directly());
        assert!(request.placement().is_some());
    }

    #[test]
    fn auto_place_maps_to_bottom_right_with_last_or_all_pages() {
        let mut request = SignRequest::new();
        auto_place(&mut request).right_mm = 15.5;
        request.bottom_mm = 8.0;
        assert_eq!(
            request.placement(),
            Some(Placement::BottomRight {
                pages: Pages::Last,
                right_mm: 15.5,
                bottom_mm: 8.0
            }),
            "the last page is the default"
        );
        request.last_page_only = false;
        assert_eq!(
            request.placement(),
            Some(Placement::BottomRight {
                pages: Pages::All,
                right_mm: 15.5,
                bottom_mm: 8.0
            })
        );
        // Without auto-stamping the host has no placement to offer.
        let mut request = SignRequest::new();
        request.last_page_only = false;
        request.right_mm = 30.0;
        assert_eq!(request.placement(), None);
    }

    #[test]
    fn summarize_sorts_mixed_reports() {
        use SignatureOverall::{Invalid, Unparseable, Unverified, Valid};
        assert_eq!(summarize(&[]), SignatureSummary::None, "no signature");
        assert_eq!(
            summarize(&[report("1", Valid), report("2", Valid)]),
            SignatureSummary::AllValid(2)
        );
        assert_eq!(
            summarize(&[report("1", Valid), report("2", Invalid), report("3", Valid)]),
            SignatureSummary::SomeInvalid {
                invalid: 1,
                total: 3
            },
            "a failure outranks the rest"
        );
        assert_eq!(
            summarize(&[report("1", Unverified), report("2", Unparseable)]),
            SignatureSummary::OnlyUnverified(2)
        );
        // Nothing failed and not everything could be confirmed: the count is
        // of those that could not.
        assert_eq!(
            summarize(&[report("1", Valid), report("2", Unverified)]),
            SignatureSummary::OnlyUnverified(1)
        );
        assert_eq!(
            summarize(&[
                report("1", Valid),
                report("2", Invalid),
                report("3", Unverified)
            ]),
            SignatureSummary::SomeInvalid {
                invalid: 1,
                total: 3
            }
        );
    }

    #[test]
    fn newly_invalidated_lists_only_valid_to_not_valid() {
        use SignatureOverall::{Invalid, Unverified, Valid};
        let before = [report("1", Valid), report("2", Valid), report("3", Invalid)];
        let after = [
            report("1", Valid),
            report("2", Invalid),
            report("3", Invalid),
        ];
        assert_eq!(newly_invalidated(&before, &after), vec!["2".to_owned()]);
        // Unverified is not valid either.
        let after = [
            report("1", Unverified),
            report("2", Valid),
            report("3", Invalid),
        ];
        assert_eq!(newly_invalidated(&before, &after), vec!["1".to_owned()]);
        // Nothing changes, or a signature is added.
        assert!(newly_invalidated(&before, &before).is_empty());
        let grown = [
            report("1", Valid),
            report("2", Valid),
            report("3", Invalid),
            report("4", Valid),
        ];
        assert!(newly_invalidated(&before, &grown).is_empty());
        // A signature that vanished could not have stayed valid.
        assert_eq!(
            newly_invalidated(&before, &[report("1", Valid)]),
            vec!["2".to_owned()]
        );
        // Whatever was not valid before is not reported again.
        let before = [report("1", Invalid)];
        assert!(newly_invalidated(&before, &[]).is_empty());
    }
}
