//! Print-session pieces (spec 2026-10-31 print design §2.1/§2.4-§2.6): the
//! content-layer flags ([`PrintContent`]), the print coordinate baseline
//! ([`PRINT_ZOOM`]), the page-size -> paper mapping, and the owned
//! [`PrintSession`] a host receives from `OfdComponent::begin_print`.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use imaging::record::Scene;
use office_rs_print::{Orientation, Paper, PrintPayload, PrintSettings};
use rofd_dom::{Annotation, AnnotationKind, OfdDocument};
use rofd_render::FontStore;

/// Print content flags (spec §2.1). Each layer is independent; defaults
/// are the all-on preset (watermark/annotations/seal on — convert spec D17).
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
            watermark: true,
            annotations: true,
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

/// A detached print session (spec 2026-10-31 §2.5): owns a document
/// snapshot (cheap — heavy resources are `Arc`-shared with the component),
/// a font-store clone (Rc-shared internals), the content flags, and the
/// generation counter shared with the component. Hosts iterate
/// `page_count`, read `page_rect`/`page_parameters` for paper geometry,
/// pull `page_scene(n)` for preview/spooler replay, and re-`begin_print`
/// once `is_stale` flips. Scenes are page-local at [`PRINT_ZOOM`] and
/// never carry screen chrome (caret/selection handles/scrollbars) — only
/// the body plus the content-flag-filtered annotations.
pub struct PrintSession {
    doc: OfdDocument,
    fonts: FontStore,
    content: PrintContent,
    /// Shared with the component; bumped on every annotation mutation.
    print_gen: Arc<AtomicU64>,
    /// Generation snapshot at `begin_print` — staleness compares against it.
    gen_at_begin: u64,
    /// Lazily built page-local scenes (0-based page index), keyed the same.
    page_cache: HashMap<usize, Scene>,
}

impl PrintSession {
    pub(crate) fn new(
        doc: OfdDocument,
        fonts: FontStore,
        content: PrintContent,
        print_gen: Arc<AtomicU64>,
    ) -> Self {
        let gen_at_begin = print_gen.load(Ordering::Relaxed);
        Self {
            doc,
            fonts,
            content,
            print_gen,
            gen_at_begin,
            page_cache: HashMap::new(),
        }
    }

    /// Build a session with no backing [`crate::ofd_component::OfdComponent`]
    /// (headless conversion facade, rofd convert phase 1): page scenes are
    /// built straight from a caller-owned document snapshot. Headless
    /// conversion has no concurrent mutation source, so the session owns a
    /// fresh generation counter that stays at 0 forever and [`PrintSession::is_stale`]
    /// is always false.
    pub fn headless(doc: OfdDocument, fonts: FontStore, content: PrintContent) -> Self {
        Self::new(doc, fonts, content, Arc::new(AtomicU64::new(0)))
    }

    /// Number of pages in the snapshotted document.
    pub fn page_count(&self) -> usize {
        self.doc.pages.len()
    }

    /// Page `(width, height)` in print pixels: `physical_box * PRINT_ZOOM`
    /// (spec §2.6, the 96-DPI px/mm baseline).
    pub fn page_rect(&self, n: usize) -> (f64, f64) {
        let page = &self.doc.pages[n];
        (
            page.physical_box.w * PRINT_ZOOM,
            page.physical_box.h * PRINT_ZOOM,
        )
    }

    /// Paper + orientation for page `n` (0-based): paper from the page's
    /// physical size, landscape when the page is wider than tall.
    pub fn page_parameters(&self, n: usize) -> (Paper, Orientation) {
        let page = &self.doc.pages[n];
        let orientation = if page.physical_box.w > page.physical_box.h {
            Orientation::Landscape
        } else {
            Orientation::Portrait
        };
        (
            paper_from_size_mm(page.physical_box.w, page.physical_box.h),
            orientation,
        )
    }

    /// Page-local scene (0-based index): the body page scene (with its white
    /// page base) plus the content-flag-filtered annotations drawn at origin
    /// (0,0), both at [`PRINT_ZOOM`]. Built lazily on first request, cached,
    /// and returned as a clone.
    pub fn page_scene(&mut self, n: usize) -> Scene {
        if let Some(cached) = self.page_cache.get(&n) {
            return cached.clone();
        }
        let page = &self.doc.pages[n];
        let mut scene =
            rofd_render::page_body_scene(page, &self.doc.resources, &self.fonts, PRINT_ZOOM);
        let filtered: Vec<Annotation> = self
            .doc
            .annotations
            .for_page(&page.id)
            .iter()
            .filter(|ann| self.content.includes(&ann.kind))
            .cloned()
            .collect();
        if !filtered.is_empty() {
            let mut painter = imaging::Painter::new(&mut scene);
            rofd_render::draw_annotations(
                &mut painter,
                &filtered,
                &self.doc.resources,
                &self.fonts,
                (0.0, 0.0),
                PRINT_ZOOM,
                None,
            );
        }
        self.page_cache.insert(n, scene);
        self.page_cache
            .get(&n)
            .expect("scene just inserted")
            .clone()
    }

    /// Validate a raw 1-based page list (host UI input): drop out-of-range
    /// entries, dedupe, sort ascending. An empty result (no printable page
    /// survived) is an error.
    pub fn validate_pages(&self, pages: &[u32]) -> Result<Vec<usize>, String> {
        let total = self.page_count();
        let mut out: Vec<usize> = pages
            .iter()
            .map(|&p| p as usize)
            .filter(|&p| p >= 1 && p <= total)
            .collect();
        out.sort_unstable();
        out.dedup();
        if out.is_empty() {
            Err("no printable pages".to_string())
        } else {
            Ok(out)
        }
    }

    /// Page plan: pure delegation to office-rs-print's `resolve_page_list`
    /// — hosts never reimplement the range grammar.
    pub fn plan(
        &self,
        settings: &PrintSettings,
        current_page: usize,
    ) -> Result<Vec<usize>, String> {
        office_rs_print::resolve_page_list(
            &settings.range,
            settings.parity,
            self.page_count(),
            current_page,
        )
    }

    /// Build the native submission payload (office-rs-print
    /// `PrintPayload::Pages`) for a planned 1-based page list: per page, the
    /// paper/orientation plus the page-local scene. Copies and collate stay
    /// JOB parameters — this never multiplies pages.
    pub fn payload_for(&mut self, pages: &[usize]) -> PrintPayload {
        let total = self.page_count();
        let scenes = pages
            .iter()
            .filter(|p| **p >= 1 && **p <= total)
            .map(|p| {
                let idx = p - 1;
                let (paper, orientation) = self.page_parameters(idx);
                let (width_px, height_px) = self.page_rect(idx);
                office_rs_print::PageScene {
                    paper,
                    orientation,
                    width_px: width_px as f32,
                    height_px: height_px as f32,
                    scene: office_rs_print::SceneWire(self.page_scene(idx)),
                }
            })
            .collect();
        PrintPayload::Pages { pages: scenes }
    }

    /// Whether the document changed (an annotation mutation bumped the
    /// shared generation) since `begin_print` — hosts re-`begin_print`.
    pub fn is_stale(&self) -> bool {
        self.print_gen.load(Ordering::Relaxed) != self.gen_at_begin
    }
}

#[cfg(test)]
mod tests {
    use super::{paper_from_size_mm, PrintContent, PrintSession, PRINT_ZOOM};
    use crate::config::OfdConfig;
    use crate::ofd_component::OfdComponent;
    use imaging::record::{Command, Draw};
    use rofd_dom::{
        Annotation, AnnotationId, AnnotationKind, AnnotationPayload, Color, FontId, ImageId, Layer,
        LayerType, NoteIcon, ObjectId, OfdDocument, Page, PageId, PageObject, Point, Rect,
        Resources, ShapeKind, TextCode, TextObject,
    };
    use rofd_render::FontStore;
    use std::sync::Arc;

    /// Minimal 1x1 white PNG (valid IHDR/IDAT/IEND) — stamps decode through
    /// `image::load_from_memory`, so the fixture bytes must be a real image.
    const TINY_PNG: &[u8] = &[
        137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 1, 0, 0, 0, 1, 8, 2,
        0, 0, 0, 144, 119, 83, 222, 0, 0, 0, 12, 73, 68, 65, 84, 120, 156, 99, 248, 255, 255, 63,
        0, 5, 254, 2, 254, 13, 239, 70, 184, 0, 0, 0, 0, 73, 69, 78, 68, 174, 66, 96, 130,
    ];

    /// The render test fixture font: a real TTF, so annotation text shapes
    /// into glyph runs (the same font the tooltip tests shape with).
    fn test_font() -> Arc<Vec<u8>> {
        Arc::new(include_bytes!("../../render/tests/fixtures/fonts/TestFont.ttf").to_vec())
    }

    /// One A4-portrait page (210x297mm), empty body layer.
    fn a4_page(id: &str) -> Page {
        Page {
            raw_children: Vec::new(),
            id: PageId::new(id),
            physical_box: Rect {
                x: 0.0,
                y: 0.0,
                w: 210.0,
                h: 297.0,
            },
            layers: vec![Layer::default()],
            template: None,
        }
    }

    fn highlight_ann(id: u64) -> Annotation {
        Annotation {
            id: AnnotationId::from_int(id),
            kind: AnnotationKind::Highlight,
            page: PageId::new("P0"),
            creator: "t".into(),
            created: 1,
            modified: 1,
            reply_to: None,
            payload: AnnotationPayload::Markup {
                quad_points: vec![Point { x: 10.0, y: 10.0 }, Point { x: 100.0, y: 100.0 }],
                color: Color::Rgb(255, 255, 0),
            },
        }
    }

    fn stamp_ann(id: u64) -> Annotation {
        Annotation {
            id: AnnotationId::from_int(id),
            kind: AnnotationKind::Stamp,
            page: PageId::new("P0"),
            creator: "t".into(),
            created: 1,
            modified: 1,
            reply_to: None,
            payload: AnnotationPayload::Stamp {
                rect: Rect {
                    x: 20.0,
                    y: 20.0,
                    w: 60.0,
                    h: 60.0,
                },
                image: ImageId::new("IMG-STAMP"),
            },
        }
    }

    fn watermark_ann(id: u64) -> Annotation {
        Annotation {
            id: AnnotationId::from_int(id),
            kind: AnnotationKind::Watermark,
            page: PageId::new("P0"),
            creator: "t".into(),
            created: 1,
            modified: 1,
            reply_to: None,
            payload: AnnotationPayload::Watermark {
                rect: Rect {
                    x: 50.0,
                    y: 130.0,
                    w: 110.0,
                    h: 40.0,
                },
                content: "t".into(),
                opacity: 0.5,
                angle: 0.785,
                font: FontId::new("F-wm"),
                size: 40.0,
                color: Color::Rgb(128, 128, 128),
            },
        }
    }

    /// One A4 page + a Highlight, a Stamp (with real decodable image bytes)
    /// and a Watermark; the body layer carries a TextObject only when
    /// `body_text` is set (annotation-only scenes then have no other glyph
    /// source, which the watermark toggle test relies on).
    fn annotated_doc(body_text: bool) -> OfdDocument {
        let mut doc = OfdDocument::default();
        let mut page = a4_page("P0");
        if body_text {
            page.layers[0] = Layer {
                raw_attrs: Vec::new(),
                raw_children: Vec::new(),
                layer_type: LayerType::Body,
                objects: vec![PageObject::Text(TextObject {
                    raw_attrs: Vec::new(),
                    id: ObjectId::new("t1"),
                    font: FontId::new("F-body"),
                    size: 12.0,
                    codes: vec![TextCode {
                        text: "print body".into(),
                        ..Default::default()
                    }],
                    ..Default::default()
                })],
            };
        }
        doc.pages.push(page);
        doc.annotations.insert(highlight_ann(1));
        doc.annotations.insert(stamp_ann(2));
        doc.annotations.insert(watermark_ann(3));
        doc.resources
            .images
            .insert(ImageId::new("IMG-STAMP"), Arc::new(TINY_PNG.to_vec()));
        doc
    }

    /// Component with the shared test font as default (annotation text can
    /// shape) and the given document loaded.
    fn component_with(doc: OfdDocument) -> OfdComponent {
        let mut c = OfdComponent::new_native(OfdConfig::new(test_font()));
        c.set_clock("t".into(), 1);
        c.load_document(doc);
        c
    }

    /// N plain A4 pages, no annotations.
    fn pages_component(n: usize) -> OfdComponent {
        let mut doc = OfdDocument::default();
        for i in 0..n {
            doc.pages.push(a4_page(&format!("P{i}")));
        }
        component_with(doc)
    }

    /// Two plain A4 pages, no annotations — the document fixture for the
    /// headless constructor (same setup as `pages_component`, no component).
    fn two_page_doc() -> OfdDocument {
        let mut doc = OfdDocument::default();
        doc.pages.push(a4_page("P0"));
        doc.pages.push(a4_page("P1"));
        doc
    }

    /// The scene's draw ops in command order (the tooltip tests' Command::Draw
    /// filter pattern).
    fn draws(scene: &imaging::record::Scene) -> Vec<&Draw> {
        scene
            .commands()
            .iter()
            .filter_map(|cmd| match cmd {
                Command::Draw(id) => Some(scene.draw_op(*id)),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn print_content_default_is_all_layers_on() {
        let c = PrintContent::default();
        assert!(
            c.watermark && c.annotations && c.seal,
            "D17: all layers on by default"
        );
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

    // ---- PrintSession (plan Task 3) ----

    /// Page geometry uses the print coordinate baseline, not the screen zoom.
    #[test]
    fn page_count_and_rect_use_print_zoom() {
        let c = pages_component(2);
        let session = c.begin_print(PrintContent::default());
        assert_eq!(session.page_count(), 2);
        let (w, h) = session.page_rect(0);
        assert_eq!(w, 210.0 * PRINT_ZOOM);
        assert_eq!(h, 297.0 * PRINT_ZOOM);
    }

    /// With every content layer off, the page scene's draws equal the
    /// `page_body_scene` baseline computed independently: Highlight, Stamp
    /// and Watermark are all filtered out while the body survives.
    #[test]
    fn annotations_off_yields_body_only_scene() {
        let doc = annotated_doc(true);
        let c = component_with(doc.clone());
        let fonts = FontStore::from_resources(&doc.resources, test_font());
        let baseline =
            rofd_render::page_body_scene(&doc.pages[0], &doc.resources, &fonts, PRINT_ZOOM);

        let all_off = PrintContent {
            watermark: false,
            annotations: false,
            seal: false,
        };
        let mut session = c.begin_print(all_off);
        assert_eq!(
            draws(&session.page_scene(0)).len(),
            draws(&baseline).len(),
            "all annotation layers off -> draw count == body-only baseline"
        );
    }

    /// Default content keeps the seal: the Stamp decodes into an image-brush
    /// fill; seal off removes that draw.
    #[test]
    fn seal_on_keeps_stamp_image_draw() {
        let c = component_with(annotated_doc(false));
        let image_fills = |scene: &imaging::record::Scene| {
            draws(scene)
                .iter()
                .filter(|d| {
                    matches!(
                        d,
                        Draw::Fill {
                            brush: imaging::peniko::Brush::Image(_),
                            ..
                        }
                    )
                })
                .count()
        };
        let mut seal_on = c.begin_print(PrintContent::default());
        assert!(
            PrintContent::default().seal,
            "default preset keeps the seal"
        );
        assert_eq!(
            image_fills(&seal_on.page_scene(0)),
            1,
            "seal on: stamp decodes to an image-brush fill"
        );
        let seal_off = PrintContent {
            seal: false,
            ..PrintContent::default()
        };
        let mut seal_off_session = c.begin_print(seal_off);
        assert_eq!(
            image_fills(&seal_off_session.page_scene(0)),
            0,
            "seal off: the stamp draw disappears"
        );
    }

    /// Watermark on adds glyph runs; off removes them (the fixture page has
    /// no body text, so the watermark is the only glyph source).
    #[test]
    fn watermark_toggle_controls_watermark_glyphs() {
        let c = component_with(annotated_doc(false));
        let has_glyph_run = |scene: &imaging::record::Scene| {
            draws(scene).iter().any(|d| matches!(d, Draw::GlyphRun(_)))
        };

        let watermark_on = PrintContent {
            watermark: true,
            ..PrintContent::default()
        };
        let mut on = c.begin_print(watermark_on);
        assert!(
            has_glyph_run(&on.page_scene(0)),
            "watermark on -> glyph run drawn"
        );

        let watermark_off = PrintContent {
            watermark: false,
            ..PrintContent::default()
        };
        let mut off = c.begin_print(watermark_off);
        assert!(
            !has_glyph_run(&off.page_scene(0)),
            "watermark off -> no glyph run"
        );
    }

    /// validate_pages: 1-based, out-of-range dropped, deduped ascending;
    /// an empty result is an error.
    #[test]
    fn validate_pages_dedupes_sorts_and_rejects_empty() {
        let c = pages_component(3);
        let session = c.begin_print(PrintContent::default());
        assert_eq!(
            session.validate_pages(&[3, 1, 1, 99, 2]).unwrap(),
            vec![1, 2, 3]
        );
        assert!(
            session.validate_pages(&[9]).is_err(),
            "all out of range -> Err"
        );
        assert!(session.validate_pages(&[]).is_err(), "empty input -> Err");
    }

    /// plan delegates range/parity to the office-rs-print grammar.
    #[test]
    fn plan_delegates_range_and_parity() {
        use office_rs_print::{PageRange, Parity, PrintSettings};
        let c = pages_component(5);
        let session = c.begin_print(PrintContent::default());
        let settings = |range: PageRange, parity: Parity| PrintSettings {
            range,
            parity,
            ..PrintSettings::default()
        };
        assert_eq!(
            session
                .plan(&settings(PageRange::All, Parity::All), 1)
                .unwrap(),
            vec![1, 2, 3, 4, 5]
        );
        assert_eq!(
            session
                .plan(&settings(PageRange::All, Parity::Odd), 1)
                .unwrap(),
            vec![1, 3, 5]
        );
        assert_eq!(
            session
                .plan(
                    &settings(
                        PageRange::Custom {
                            spec: "1-2,5".into()
                        },
                        Parity::All
                    ),
                    1
                )
                .unwrap(),
            vec![1, 2, 5]
        );
        assert_eq!(
            session
                .plan(&settings(PageRange::Current, Parity::All), 5)
                .unwrap(),
            vec![5]
        );
        assert!(
            session
                .plan(
                    &settings(PageRange::Custom { spec: "x".into() }, Parity::All),
                    1
                )
                .is_err(),
            "garbage spec -> Err"
        );
    }

    /// The session stays fresh until an annotation mutation bumps the shared
    /// generation (the mutation goes through the component so the single
    /// bump point in notify_document_change fires).
    #[test]
    fn is_stale_flips_after_annotation_mutation() {
        let mut c = component_with(annotated_doc(false));
        let session = c.begin_print(PrintContent::default());
        assert!(!session.is_stale(), "fresh session is not stale");

        c.create_annotation(
            AnnotationKind::Note,
            PageId::new("P0"),
            AnnotationPayload::Note {
                rect: Rect {
                    x: 10.0,
                    y: 10.0,
                    w: 20.0,
                    h: 20.0,
                },
                color: Color::Rgb(255, 255, 0),
                content: "n".into(),
                icon: NoteIcon::Note,
            },
        );
        assert!(
            session.is_stale(),
            "annotation mutation flags the session stale"
        );
    }

    /// A document swap invalidates detached sessions: after `load_document`
    /// replaces the document, a session begun on the old one reports stale.
    #[test]
    fn doc_swap_flags_session_stale() {
        let mut c = pages_component(1);
        let session = c.begin_print(PrintContent::default());
        assert!(!session.is_stale(), "fresh session is not stale");

        let mut doc = OfdDocument::default();
        doc.pages.push(a4_page("Q0"));
        doc.pages.push(a4_page("Q1"));
        c.load_document(doc);
        assert!(session.is_stale(), "load_document flags the session stale");
    }

    /// Same staleness contract for the reset path: `new_document` must also
    /// flag a session begun on the previous document stale.
    #[test]
    fn new_document_flags_session_stale() {
        let mut c = pages_component(1);
        let session = c.begin_print(PrintContent::default());
        assert!(!session.is_stale(), "fresh session is not stale");

        c.new_document();
        assert!(session.is_stale(), "new_document flags the session stale");
    }

    /// payload_for packs exactly the planned pages with paper from
    /// physical_box and a recorded (non-empty) scene.
    #[test]
    fn payload_for_packs_planned_pages_only() {
        let c = pages_component(3);
        let mut session = c.begin_print(PrintContent::default());
        let payload = session.payload_for(&[1, 3]);
        match payload {
            office_rs_print::PrintPayload::Pages { pages } => {
                assert_eq!(pages.len(), 2, "only the planned pages");
                assert_eq!(pages[0].paper, office_rs_print::Paper::A4);
                assert_eq!(pages[0].orientation, office_rs_print::Orientation::Portrait);
                assert!((pages[0].width_px - (210.0 * PRINT_ZOOM) as f32).abs() < 0.5);
                assert!((pages[0].height_px - (297.0 * PRINT_ZOOM) as f32).abs() < 0.5);
                assert!(!pages[0].scene.0.is_empty(), "page scene recorded");
                assert!(!pages[1].scene.0.is_empty(), "second page scene recorded");
            }
            _ => panic!("wrong payload variant"),
        }
    }

    /// The headless constructor (convert facade): builds page scenes with no
    /// backing `OfdComponent`, and with no component able to bump the shared
    /// generation the session never goes stale.
    #[test]
    fn headless_session_renders_pages_and_never_stale() {
        let fonts = FontStore::from_resources(&Resources::default(), Arc::new(vec![]));
        let mut session = PrintSession::headless(two_page_doc(), fonts, PrintContent::default());
        assert_eq!(session.page_count(), 2);
        let _ = session.page_scene(0); // building must not panic
        assert!(!session.is_stale(), "no component can bump the generation");
    }
}
