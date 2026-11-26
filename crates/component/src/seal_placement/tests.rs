//! Unit tests of the modal seal placement state.
use super::*;
use crate::event::{EventOutcome, Key, Modifiers, MouseButton, ViewEvent};
use crate::ofd_component::{CreateKind, OfdComponent, Tool};
use crate::{AnnotationSelection, ContextTarget};
use imaging::peniko::Brush;
use imaging::record::{Command, Draw};
use rofd_dom::{
    AnnotationKind, AnnotationPayload, Color, Layer, OfdDocument, Page, PageId, Point, Rect,
};
use std::sync::{Arc, Mutex};

const SEAL: f64 = 40.0;

/// A 4x4 opaque red PNG. The on-screen size comes from the millimetres,
/// not from the pixels.
fn png() -> Arc<Vec<u8>> {
    let mut bytes = std::io::Cursor::new(Vec::new());
    image::RgbaImage::from_pixel(4, 4, image::Rgba([200, 30, 30, 255]))
        .write_to(&mut bytes, image::ImageFormat::Png)
        .expect("PNG");
    Arc::new(bytes.into_inner())
}

fn preview() -> SealPreview {
    SealPreview {
        png: png(),
        width_mm: SEAL,
        height_mm: SEAL,
    }
}

/// A 200 x 200 mm page, ID `P0`.
fn page_document() -> OfdDocument {
    page_document_of(200.0, 200.0)
}

fn page_document_of(width: f64, height: f64) -> OfdDocument {
    let mut doc = OfdDocument::default();
    doc.pages.push(Page {
        id: PageId::new("P0"),
        physical_box: Rect {
            x: 0.0,
            y: 0.0,
            w: width,
            h: height,
        },
        layers: vec![Layer::default()],
        template: None,
        raw_children: Vec::new(),
    });
    doc
}

/// The page at the viewport origin, zoom 1, in a 200 x 200 viewport.
fn component() -> OfdComponent {
    component_with(page_document())
}

fn component_with(document: OfdDocument) -> OfdComponent {
    let mut c = OfdComponent::new_native(crate::OfdConfig::new(Arc::new(vec![])));
    c.set_clock("t".into(), 1);
    c.load_document(document);
    c.viewport = rofd_render::Viewport {
        scroll: (0.0, 0.0),
        zoom: 1.0,
        size: (200.0, 200.0),
        page_gap: 0.0,
    };
    c
}

fn move_to(c: &mut OfdComponent, x: f64, y: f64) -> EventOutcome {
    c.handle_event(&ViewEvent::PointerMove { x, y })
}

fn press(c: &mut OfdComponent, button: MouseButton, x: f64, y: f64) -> EventOutcome {
    c.handle_event(&ViewEvent::PointerDown {
        button,
        x,
        y,
        modifiers: Modifiers::default(),
        click_count: 1,
    })
}

fn key(c: &mut OfdComponent, key: Key, modifiers: Modifiers) -> EventOutcome {
    c.handle_event(&ViewEvent::KeyDown { key, modifiers })
}

/// What the two callbacks received.
#[derive(Clone, Default)]
struct Seen {
    placed: Arc<Mutex<Vec<SealPlacement>>>,
    cancelled: Arc<Mutex<usize>>,
}

impl Seen {
    fn install(&self, c: &mut OfdComponent) {
        let placed = self.placed.clone();
        c.on_seal_placed(move |placement| placed.lock().unwrap().push(placement));
        let cancelled = self.cancelled.clone();
        c.on_seal_placement_cancelled(move || *cancelled.lock().unwrap() += 1);
    }

    fn placements(&self) -> Vec<SealPlacement> {
        self.placed.lock().unwrap().clone()
    }

    fn cancellations(&self) -> usize {
        *self.cancelled.lock().unwrap()
    }
}

/// The per-draw compositing alpha of every image fill of the composed
/// scene.
fn image_fills(c: &mut OfdComponent) -> Vec<f32> {
    c.update_scene();
    let scene = c.scene();
    scene
        .commands()
        .iter()
        .filter_map(|cmd| match cmd {
            Command::Draw(id) => Some(scene.draw_op(*id)),
            _ => None,
        })
        .filter_map(|draw| match draw {
            Draw::Fill {
                brush: Brush::Image(..),
                composite,
                ..
            } => Some(composite.alpha),
            _ => None,
        })
        .collect()
}

/// The alpha channel of the preview picture.
fn preview_alphas(c: &OfdComponent) -> Vec<u8> {
    let image = c.seal_preview_image().expect("a preview picture");
    image
        .data
        .data()
        .as_chunks::<4>()
        .0
        .iter()
        .map(|pixel| pixel[3])
        .collect()
}

#[test]
fn left_click_reports_placement_then_exits() {
    let mut c = component();
    let before = Tool::Create(CreateKind::Shape(rofd_dom::ShapeKind::Rect));
    c.set_tool(before.clone());
    let seen = Seen::default();
    seen.install(&mut c);
    assert!(!c.seal_placement_active());
    assert!(
        c.begin_seal_placement(preview()),
        "a decodable PNG is accepted"
    );
    assert!(c.seal_placement_active());

    // The host may switch tools meanwhile; the one from before entering
    // is the one that comes back.
    c.set_tool(Tool::Hand);
    assert!(c.seal_placement_active(), "a tool switch does not end it");

    assert!(
        move_to(&mut c, 100.0, 100.0).needs_repaint,
        "the preview moves"
    );
    press(&mut c, MouseButton::Left, 100.0, 100.0);
    let placed = seen.placements();
    assert_eq!(placed.len(), 1);
    assert_eq!(placed[0].page_index, 0);
    assert_eq!(placed[0].page_id, PageId::new("P0"));
    assert_eq!(
        placed[0].boundary,
        Rect {
            x: 80.0,
            y: 80.0,
            w: 40.0,
            h: 40.0
        },
        "centred on the pointer, in page millimetres"
    );
    assert!(!c.seal_placement_active());
    assert_eq!(c.tool, before, "the tool from before entering is back");
    assert_eq!(seen.cancellations(), 0);

    // The next click is an ordinary one again: nothing more is reported.
    press(&mut c, MouseButton::Left, 100.0, 100.0);
    assert_eq!(seen.placements().len(), 1);
}

#[test]
fn escape_and_right_click_cancel() {
    let mut c = component();
    let seen = Seen::default();
    seen.install(&mut c);
    let menus = Arc::new(Mutex::new(0));
    {
        let menus = menus.clone();
        c.on_context_menu(move |_: (f64, f64), _: ContextTarget| {
            *menus.lock().unwrap() += 1;
        });
    }

    c.begin_seal_placement(preview());
    move_to(&mut c, 100.0, 100.0);
    key(&mut c, Key::Escape, Modifiers::default());
    assert!(!c.seal_placement_active());
    assert_eq!(seen.cancellations(), 1);
    assert!(seen.placements().is_empty());

    c.begin_seal_placement(preview());
    press(&mut c, MouseButton::Right, 100.0, 100.0);
    assert!(!c.seal_placement_active());
    assert_eq!(seen.cancellations(), 2);
    assert_eq!(*menus.lock().unwrap(), 0, "no context menu while placing");

    // The host can end it the same way.
    c.begin_seal_placement(preview());
    c.cancel_seal_placement();
    assert!(!c.seal_placement_active());
    assert_eq!(seen.cancellations(), 3);

    // Ending what is not active does nothing.
    c.cancel_seal_placement();
    assert_eq!(seen.cancellations(), 3);
    key(&mut c, Key::Escape, Modifiers::default());
    assert_eq!(seen.cancellations(), 3);
}

#[test]
fn document_swap_exits_silently() {
    let mut c = component();
    let seen = Seen::default();
    seen.install(&mut c);
    c.begin_seal_placement(preview());
    c.load_document(page_document());
    assert!(!c.seal_placement_active());
    c.begin_seal_placement(preview());
    c.new_document();
    assert!(!c.seal_placement_active());
    assert_eq!(seen.cancellations(), 0, "nothing is reported");
    assert!(seen.placements().is_empty());
}

#[test]
fn annotation_interaction_is_intercepted_while_placing() {
    let mut c = component();
    let id = c.editor.create_annotation(
        AnnotationKind::Highlight,
        PageId::new("P0"),
        AnnotationPayload::Markup {
            quad_points: vec![Point { x: 10.0, y: 10.0 }, Point { x: 100.0, y: 100.0 }],
            color: Color::Rgb(255, 255, 0),
        },
    );
    // Selected before the seal placement begins.
    c.editor
        .set_selection(AnnotationSelection::Single(id.clone()));

    let seen = Seen::default();
    seen.install(&mut c);
    let others = Arc::new(Mutex::new(0));
    {
        let others = others.clone();
        c.on_change(move |_| *others.lock().unwrap() += 1);
    }
    {
        let others = others.clone();
        c.on_annotation_focus(move |_| *others.lock().unwrap() += 1);
    }
    {
        let others = others.clone();
        c.on_context_menu(move |_: (f64, f64), _: ContextTarget| *others.lock().unwrap() += 1);
    }
    {
        let others = others.clone();
        c.on_save_request(move || *others.lock().unwrap() += 1);
    }

    c.begin_seal_placement(preview());
    // Click on the annotation, with and without Ctrl, double too.
    press(&mut c, MouseButton::Left, 50.0, 50.0);
    assert_eq!(seen.placements().len(), 1, "only the placement is reported");
    assert_eq!(
        c.selection(),
        &AnnotationSelection::Single(id.clone()),
        "the selection is as it was"
    );
    assert_eq!(
        c.document().annotations.for_page(&PageId::new("P0")).len(),
        1
    );
    assert_eq!(*others.lock().unwrap(), 0, "no other callback fired");

    // Again: keys and the right button do nothing to the document.
    c.begin_seal_placement(preview());
    key(&mut c, Key::Delete, Modifiers::default());
    key(
        &mut c,
        Key::Char('s'),
        Modifiers {
            control: true,
            ..Default::default()
        },
    );
    key(&mut c, Key::Char('x'), Modifiers::default());
    c.handle_event(&ViewEvent::PointerUp {
        button: MouseButton::Left,
        x: 50.0,
        y: 50.0,
    });
    assert_eq!(
        c.document().annotations.for_page(&PageId::new("P0")).len(),
        1,
        "Delete did not delete"
    );
    assert!(c.seal_placement_active());
    press(&mut c, MouseButton::Right, 50.0, 50.0);
    assert!(!c.seal_placement_active());
    assert_eq!(*others.lock().unwrap(), 0, "no menu, no save, no change");
    assert_eq!(seen.cancellations(), 1);
}

#[test]
fn boundary_is_clamped_inside_page() {
    let mut c = component();
    let seen = Seen::default();
    seen.install(&mut c);

    c.begin_seal_placement(preview());
    move_to(&mut c, 5.0, 5.0);
    press(&mut c, MouseButton::Left, 5.0, 5.0);
    assert_eq!(
        seen.placements()[0].boundary,
        Rect {
            x: 0.0,
            y: 0.0,
            w: 40.0,
            h: 40.0
        },
        "the whole seal stays on the page"
    );

    c.begin_seal_placement(preview());
    move_to(&mut c, 198.0, 198.0);
    press(&mut c, MouseButton::Left, 198.0, 198.0);
    assert_eq!(
        seen.placements()[1].boundary,
        Rect {
            x: 160.0,
            y: 160.0,
            w: 40.0,
            h: 40.0
        }
    );

    // Zoomed in: still page millimetres.
    c.viewport.zoom = 2.0;
    let (origin_x, origin_y) =
        rofd_render::page_origin(c.document(), &c.viewport, 0).expect("origin");
    c.begin_seal_placement(preview());
    move_to(&mut c, origin_x + 120.0, origin_y + 120.0);
    press(
        &mut c,
        MouseButton::Left,
        origin_x + 120.0,
        origin_y + 120.0,
    );
    assert_eq!(
        seen.placements()[2].boundary,
        Rect {
            x: 40.0,
            y: 40.0,
            w: 40.0,
            h: 40.0
        },
        "60 mm from the page corner, less half the seal"
    );
}

#[test]
fn scroll_and_zoom_still_work_while_placing() {
    // A page taller than the viewport leaves room to scroll.
    let mut c = component_with(page_document_of(200.0, 600.0));
    let seen = Seen::default();
    seen.install(&mut c);
    c.begin_seal_placement(preview());

    c.handle_event(&ViewEvent::Scroll { dx: 0.0, dy: 50.0 });
    assert_eq!(c.viewport.scroll.1, 50.0, "the wheel scrolls");
    assert!(c.seal_placement_active());
    c.handle_event(&ViewEvent::Zoom { factor: 2.0 });
    assert_eq!(c.viewport.zoom, 2.0, "zooming works");
    assert!(c.seal_placement_active());
    c.handle_event(&ViewEvent::Resize {
        width: 300.0,
        height: 300.0,
    });
    assert_eq!(c.viewport.size, (300.0, 300.0));
    c.handle_event(&ViewEvent::ZoomAt {
        factor: 0.5,
        center: (150.0, 150.0),
    });
    assert_eq!(c.viewport.zoom, 1.0);
    assert!(c.seal_placement_active());

    // After all that, a click is read in page millimetres.
    let (origin_x, origin_y) =
        rofd_render::page_origin(c.document(), &c.viewport, 0).expect("origin");
    press(&mut c, MouseButton::Left, origin_x + 60.0, origin_y + 60.0);
    let placed = seen.placements();
    assert_eq!(placed.len(), 1);
    assert_eq!(
        placed[0].boundary,
        Rect {
            x: 40.0,
            y: 40.0,
            w: 40.0,
            h: 40.0
        }
    );
}

#[test]
fn a_click_off_the_pages_does_nothing() {
    let mut c = component();
    // A page narrower than the viewport leaves desk on both sides.
    c.viewport.size = (600.0, 200.0);
    let seen = Seen::default();
    seen.install(&mut c);
    c.begin_seal_placement(preview());
    press(&mut c, MouseButton::Left, 10.0, 10.0);
    assert!(seen.placements().is_empty(), "the desk takes no seal");
    assert!(c.seal_placement_active());
    assert_eq!(seen.cancellations(), 0);
}

#[test]
fn begin_with_undecodable_png_returns_false() {
    let mut c = component();
    let seen = Seen::default();
    seen.install(&mut c);
    let undecodable = |png: Vec<u8>| SealPreview {
        png: Arc::new(png),
        width_mm: SEAL,
        height_mm: SEAL,
    };
    assert!(!c.begin_seal_placement(undecodable(b"not a png".to_vec())));
    assert!(!c.begin_seal_placement(undecodable(Vec::new())));
    assert!(!c.seal_placement_active());
    assert_eq!(seen.cancellations(), 0);

    // Beginning again replaces what was going on.
    c.begin_seal_placement(preview());
    assert!(c.begin_seal_placement(SealPreview {
        png: png(),
        width_mm: 20.0,
        height_mm: 20.0
    }));
    press(&mut c, MouseButton::Left, 100.0, 100.0);
    assert_eq!(seen.placements()[0].boundary.w, 20.0, "the newer preview");
    assert_eq!(seen.cancellations(), 0, "replacing is not cancelling");
}

#[test]
fn the_preview_follows_the_pointer_and_hides_the_tooltip() {
    let mut c = component();
    c.editor.create_annotation(
        AnnotationKind::Highlight,
        PageId::new("P0"),
        AnnotationPayload::Markup {
            quad_points: vec![Point { x: 10.0, y: 10.0 }, Point { x: 100.0, y: 100.0 }],
            color: Color::Rgb(255, 255, 0),
        },
    );
    c.set_tooltip_formatter(|_| vec!["a note".to_owned()]);
    move_to(&mut c, 50.0, 50.0);
    assert!(
        c.tooltip_lines().is_some(),
        "the tooltip shows outside the placement state"
    );

    assert_eq!(image_fills(&mut c), Vec::<f32>::new(), "no preview yet");
    c.begin_seal_placement(preview());
    assert!(c.tooltip_lines().is_none(), "no tooltip while placing");
    assert!(
        image_fills(&mut c).is_empty(),
        "not until the pointer is on a page"
    );

    move_to(&mut c, 100.0, 100.0);
    assert_eq!(
        image_fills(&mut c),
        vec![1.0],
        "one image draw, with no compositing on it"
    );
    // The opacity is in the picture itself: the CPU fallback renderer
    // refuses opacity on an image draw (vello_cpu: "not implemented:
    // Applying opacity to image commands").
    assert_eq!(preview_alphas(&c), vec![153; 16], "opaque at 60%");
    // Where it is drawn: centred on the pointer.
    assert_eq!(
        c.seal_preview_rect(),
        Some(Rect {
            x: 80.0,
            y: 80.0,
            w: 40.0,
            h: 40.0
        })
    );
    // Pushed to the corner, it stays on the page.
    move_to(&mut c, 5.0, 5.0);
    assert_eq!(
        c.seal_preview_rect(),
        Some(Rect {
            x: 0.0,
            y: 0.0,
            w: 40.0,
            h: 40.0
        })
    );

    press(&mut c, MouseButton::Left, 5.0, 5.0);
    assert!(image_fills(&mut c).is_empty(), "gone once placed");
}

#[test]
fn boundary_is_reported_to_a_thousandth_of_a_millimetre() {
    // What is reported is written into the signature as text: no float
    // noise.
    let mut c = component();
    c.viewport.zoom = 3.7;
    let seen = Seen::default();
    seen.install(&mut c);
    c.begin_seal_placement(preview());
    press(&mut c, MouseButton::Left, 123.0, 77.0);
    let boundary = seen.placements()[0].boundary;
    assert_eq!(boundary.x, 13.243, "123 px / 3.7 - half the seal");
    assert_eq!(boundary.y, 0.811, "77 px / 3.7 - half the seal");
}
