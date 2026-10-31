//! Print-session pure pieces (spec 2026-10-31 print design §2.1/§2.4/§2.6):
//! the content-layer flags ([`PrintContent`]), the print coordinate
//! baseline ([`PRINT_ZOOM`]), and the page-size -> paper mapping. The
//! owned `PrintSession` itself is a later slice; only side-effect-free
//! helpers live here.

use rofd_dom::AnnotationKind;

/// Print content flags (spec §2.1). Each layer is independent; defaults
/// are the "clean copy" preset (watermark/annotations off, seal on).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct PrintContent {
    /// `AnnotationKind::Watermark` layer.
    pub watermark: bool,
    /// Every other annotation kind (§2.4 mapping table).
    pub annotations: bool,
    /// `AnnotationKind::Stamp` layer (v1 image-stamp semantics).
    pub seal: bool,
}

impl Default for PrintContent {
    fn default() -> Self {
        Self {
            watermark: false,
            annotations: false,
            seal: true,
        }
    }
}

impl PrintContent {
    /// Keep annotations of this kind? (§2.4 mapping: Watermark ->
    /// `watermark`, Stamp -> `seal`, all other kinds -> `annotations`.)
    pub fn includes(&self, kind: &AnnotationKind) -> bool {
        match kind {
            AnnotationKind::Watermark => self.watermark,
            AnnotationKind::Stamp => self.seal,
            _ => self.annotations,
        }
    }
}

/// Print coordinate baseline: the same 96-DPI px/mm convention as the
/// screen viewport (§2.6). PDF encoding converts px->pt (×0.75) so
/// mm->pt lands exactly on 72/25.4.
pub const PRINT_ZOOM: f64 = rofd_render::PX_PER_MM;

/// Map a portrait-normalized page size (mm) onto the v1
/// [`office_rs_print::Paper`] set: exact within ±1mm, else the closest by
/// area (spec §2.4 "actual size" behavior — unmatched custom sizes keep a
/// sane paper guess; table copied from rword `print_session.rs`).
/// Consumed by the print session (next slice) — staged here first.
#[allow(dead_code)]
pub(crate) fn paper_from_size_mm(w_mm: f64, h_mm: f64) -> office_rs_print::Paper {
    use office_rs_print::Paper;
    let (short, long) = if w_mm <= h_mm {
        (w_mm, h_mm)
    } else {
        (h_mm, w_mm)
    };
    let table = [
        (Paper::A3, 297.0, 420.0),
        (Paper::A4, 210.0, 297.0),
        (Paper::A5, 148.0, 210.0),
        (Paper::B4, 250.0, 353.0),
        (Paper::B5, 176.0, 250.0),
        (Paper::Letter, 215.9, 279.4),
        (Paper::Legal, 215.9, 355.6),
    ];
    for (paper, t_short, t_long) in table {
        if (short - t_short).abs() <= 1.0 && (long - t_long).abs() <= 1.0 {
            return paper;
        }
    }
    // Closest by area.
    table
        .into_iter()
        .min_by(|a, b| {
            let area = |t: (_, f64, f64)| t.1 * t.2;
            let target = short * long;
            (area(*a) - target)
                .abs()
                .partial_cmp(&(area(*b) - target).abs())
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .map(|(paper, _, _)| paper)
        .unwrap_or(Paper::A4)
}

#[cfg(test)]
mod tests {
    use super::{paper_from_size_mm, PrintContent, PRINT_ZOOM};
    use rofd_dom::{AnnotationKind, ShapeKind};

    #[test]
    fn print_content_default_is_clean_copy_with_seal() {
        let d = PrintContent::default();
        assert!(!d.watermark && !d.annotations && d.seal);
    }

    #[test]
    fn includes_maps_all_ten_kinds() {
        // Spec 2026-10-31 §2.4: Watermark -> `watermark`, Stamp -> `seal`,
        // every other kind -> `annotations`.
        let ann_only = PrintContent {
            watermark: false,
            annotations: true,
            seal: false,
        };
        let ann_kinds = [
            AnnotationKind::Highlight,
            AnnotationKind::Underline,
            AnnotationKind::Strikeout,
            AnnotationKind::Squiggly,
            AnnotationKind::Freehand,
            AnnotationKind::Shape(ShapeKind::Rect),
            AnnotationKind::Note,
            AnnotationKind::TextBox,
        ];
        for kind in ann_kinds {
            assert!(
                ann_only.includes(&kind),
                "{kind:?} must map to `annotations`"
            );
        }
        assert!(!ann_only.includes(&AnnotationKind::Watermark));
        assert!(!ann_only.includes(&AnnotationKind::Stamp));

        let wm_only = PrintContent {
            watermark: true,
            annotations: false,
            seal: false,
        };
        assert!(wm_only.includes(&AnnotationKind::Watermark));
        assert!(!wm_only.includes(&AnnotationKind::Stamp));
        assert!(!wm_only.includes(&AnnotationKind::Highlight));

        let seal_only = PrintContent {
            watermark: false,
            annotations: false,
            seal: true,
        };
        assert!(seal_only.includes(&AnnotationKind::Stamp));
        assert!(!seal_only.includes(&AnnotationKind::Watermark));
        assert!(!seal_only.includes(&AnnotationKind::TextBox));
    }

    #[test]
    fn paper_table_exact_and_normalized_and_closest() {
        use office_rs_print::Paper;
        assert_eq!(paper_from_size_mm(210.0, 297.0), Paper::A4);
        assert_eq!(paper_from_size_mm(297.0, 210.0), Paper::A4); // landscape normalizes
        assert_eq!(paper_from_size_mm(215.9, 279.4), Paper::Letter); // inside ±1mm
        assert_eq!(paper_from_size_mm(300.0, 430.0), Paper::A3); // closest by area
    }

    #[test]
    fn print_zoom_is_px_per_mm() {
        assert_eq!(PRINT_ZOOM, rofd_render::PX_PER_MM);
    }
}
