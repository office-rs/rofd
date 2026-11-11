// Ported from rword@4e0b25b6 (crates/gpui-view/tests/view_input.rs) —
// rofd divergences: the test corpus is built through rofd-dom types +
// the OfdComponent pass-throughs (rword loaded an rword-dom Document
// directly), and the IME/undo/clipboard scenarios drive a TextBox
// annotation and body text instead of paragraph spans.

//! Input-channel tests for the completed view surface.
//!
//! Everything runs headless (TestAppContext/VisualTestContext) and
//! asserts on kernel state — selection, document content, clipboard —
//! never pixels:
//! - mouse listeners translate window coords into viewport coords by
//!   the element origin (a padded wrapper forces a non-zero origin);
//! - plain typed characters insert exactly once (gpui delivers printable
//!   input on BOTH the raw-key and the InputHandler channels; only the
//!   InputHandler channel may insert);
//! - `replace_and_mark_text_in_range` + `unmark_text` (the TSF
//!   confirm-as-is flavor) commits the marked text into the annotation;
//! - ctrl+z / ctrl+shift+z are adapter-intercepted undo/redo;
//! - ctrl+c writes the body-text selection to the clipboard.

use std::sync::Arc;

use gpui::{
    div, point, prelude::*, px, size, Entity, Modifiers, MouseButton, Render, TestAppContext,
    VisualTestContext, WindowHandle,
};
use rofd_component::{AnnotationSelection, OfdConfig};
use rofd_dom::{
    AnnotationKind, AnnotationPayload, Color, FontId, Layer, LayerType, NoteIcon, ObjectId,
    OfdDocument, Page, PageId, PageObject, Rect, TextCode, TextObject,
};
use rofd_gpui_view::OfdGpuiView;

/// Wrapper padding that forces a known non-zero element origin: the
/// editor element sits at window (PAD_X, PAD_Y), and every mouse event
/// must be translated by exactly that before it reaches the kernel.
const PAD_X: f32 = 40.0;
const PAD_Y: f32 = 24.0;
const WIN_W: f32 = 800.0;
const WIN_H: f32 = 600.0;
/// Page size in mm; every test runs at zoom 1.0 (1 px per mm).
const PAGE_MM: f64 = 200.0;
/// Default `OfdConfig` page gap (px).
const PAGE_GAP: f64 = 20.0;

fn test_config() -> OfdConfig {
    OfdConfig::new(Arc::new(vec![])).with_zoom(1.0)
}

/// One empty 200x200 mm page.
fn page_doc() -> OfdDocument {
    let mut doc = OfdDocument::default();
    doc.pages.push(Page {
        raw_children: Vec::new(),
        id: PageId::new("P0"),
        physical_box: Rect {
            x: 0.0,
            y: 0.0,
            w: PAGE_MM,
            h: PAGE_MM,
        },
        layers: vec![Layer::default()],
        template: None,
    });
    doc
}

/// A page with two lines of body text ("ABCD" / "EF", 10 mm glyph cells
/// in a text object at page (10, 20)) — the same corpus the kernel's own
/// body-text selection tests use.
fn body_text_doc() -> OfdDocument {
    let mut doc = page_doc();
    doc.pages[0].layers = vec![Layer {
        raw_attrs: Vec::new(),
        raw_children: Vec::new(),
        layer_type: LayerType::Body,
        objects: vec![PageObject::Text(TextObject {
            raw_attrs: Vec::new(),
            id: ObjectId::new("t1"),
            boundary: Rect {
                x: 10.0,
                y: 20.0,
                w: 100.0,
                h: 40.0,
            },
            ctm: None,
            font: FontId::new("F1"),
            size: 10.0,
            fill: None,
            codes: vec![
                TextCode {
                    glyph_ids: vec![1, 2, 3, 4],
                    deltas: vec![(10.0, 0.0), (10.0, 0.0), (10.0, 0.0)],
                    text: "ABCD".into(),
                    x: 0.0,
                    y: 10.0,
                },
                TextCode {
                    glyph_ids: vec![5, 6],
                    deltas: vec![(10.0, 0.0)],
                    text: "EF".into(),
                    x: 0.0,
                    y: 30.0,
                },
            ],
            draw_param: None,
        })],
    }];
    doc
}

/// Page 0's origin in viewport px at scroll 0 (the kernel's page-stacking
/// math: horizontally centered, one gap below the top).
fn page_origin(viewport_w: f64) -> (f64, f64) {
    (((viewport_w - PAGE_MM) / 2.0).max(0.0), PAGE_GAP)
}

/// The editor filling the whole window (element origin (0, 0)) — the
/// setup for every test that does not exercise origin translation.
fn open_editor(
    cx: &mut TestAppContext,
    configure: impl FnOnce(&mut OfdGpuiView),
) -> (WindowHandle<OfdGpuiView>, VisualTestContext) {
    let window = cx.open_window(size(px(WIN_W), px(WIN_H)), |_, cx| {
        let mut view = OfdGpuiView::new(test_config(), cx);
        configure(&mut view);
        view
    });
    let vcx = VisualTestContext::from_window(window.into(), cx);
    (window, vcx)
}

/// Keystrokes dispatch to the focused element — focus the editor first
/// (also lets the paint register the InputHandler while focused).
fn focus_editor(window: &WindowHandle<OfdGpuiView>, vcx: &mut VisualTestContext) {
    let focus = window
        .update(vcx, |view, _, _| view.focus_handle.clone())
        .unwrap();
    window
        .update(vcx, |_, window, cx| window.focus(&focus, cx))
        .unwrap();
}

/// Add a TextBox annotation with content "Hello" at page (10, 10).
fn add_textbox(view: &mut OfdGpuiView) {
    view.component.create_annotation(
        AnnotationKind::TextBox,
        PageId::new("P0"),
        AnnotationPayload::TextBox {
            rect: Rect {
                x: 10.0,
                y: 10.0,
                w: 100.0,
                h: 40.0,
            },
            content: "Hello".into(),
            font: FontId::new("F1"),
            size: 10.0,
            color: Color::Rgb(0, 0, 0),
            border: None,
            spans: Vec::new(),
        },
    );
}

fn textbox_content(view: &OfdGpuiView) -> String {
    let ann = view
        .component
        .document()
        .annotations
        .for_page(&PageId::new("P0"))
        .first()
        .expect("one textbox annotation");
    match &ann.payload {
        AnnotationPayload::TextBox { content, .. } => content.clone(),
        other => panic!("expected textbox, got {other:?}"),
    }
}

/// Click the textbox body: page-local (60, 30) translated through the
/// page origin of a full-window editor. Positions the cursor at the
/// content end (kernel click behavior for text annotations).
fn click_textbox(vcx: &mut VisualTestContext) {
    let (page_x, page_y) = page_origin(WIN_W as f64);
    let pos = point(px(page_x as f32 + 60.0), px(page_y as f32 + 30.0));
    vcx.simulate_mouse_down(pos, MouseButton::Left, Modifiers::default());
    vcx.simulate_mouse_up(pos, MouseButton::Left, Modifiers::default());
}

#[gpui::test]
fn mouse_down_translates_window_to_viewport(cx: &mut TestAppContext) {
    // The editor sits inside a padded wrapper: its element origin is
    // (PAD_X, PAD_Y) in window coordinates, and only the translated
    // (viewport) point may reach the kernel.
    struct PaddedHost {
        editor: Entity<OfdGpuiView>,
    }
    impl Render for PaddedHost {
        fn render(
            &mut self,
            _window: &mut gpui::Window,
            _cx: &mut gpui::Context<Self>,
        ) -> impl IntoElement {
            div()
                .size_full()
                .pl(px(PAD_X))
                .pt(px(PAD_Y))
                .child(self.editor.clone())
        }
    }

    let window = cx.open_window(size(px(WIN_W), px(WIN_H)), |_, cx| {
        let editor = cx.new(|cx| {
            let mut view = OfdGpuiView::new(test_config(), cx);
            view.component.set_clock("t".into(), 1);
            view.component.load_document(page_doc());
            view.component.create_annotation(
                AnnotationKind::Note,
                PageId::new("P0"),
                AnnotationPayload::Note {
                    rect: Rect {
                        x: 10.0,
                        y: 10.0,
                        w: 80.0,
                        h: 60.0,
                    },
                    color: Color::Rgb(255, 200, 0),
                    content: "note".into(),
                    icon: NoteIcon::Note,
                },
            );
            view
        });
        PaddedHost { editor }
    });
    let mut vcx = VisualTestContext::from_window(window.into(), cx);

    let note_id = window
        .update(&mut vcx, |host, _, cx| {
            host.editor.update(cx, |view, _| {
                view.component
                    .document()
                    .annotations
                    .for_page(&PageId::new("P0"))
                    .first()
                    .expect("one note annotation")
                    .id
                    .clone()
            })
        })
        .unwrap();
    let selection = |vcx: &mut VisualTestContext| {
        window
            .update(vcx, |host, _, cx| {
                host.editor
                    .update(cx, |view, _| view.component.selection().clone())
            })
            .unwrap()
    };

    // Miss first: window (140, 124) translates to viewport (100, 100),
    // left of the page — nothing may be selected.
    vcx.simulate_mouse_down(
        point(px(140.), px(124.)),
        MouseButton::Left,
        Modifiers::default(),
    );
    assert_eq!(
        selection(&mut vcx),
        AnnotationSelection::None,
        "a miss must not select"
    );

    // Hit: the note's viewport rect is page_origin + (10, 10, 80, 60) mm;
    // clicking its CENTER in window coordinates must select it.
    let (page_x, page_y) = page_origin((WIN_W - PAD_X) as f64);
    let hit = point(
        px(page_x as f32 + 50.0 + PAD_X),
        px(page_y as f32 + 40.0 + PAD_Y),
    );
    vcx.simulate_mouse_down(hit, MouseButton::Left, Modifiers::default());
    assert!(
        matches!(
            selection(&mut vcx),
            AnnotationSelection::Single(id) if id == note_id
        ),
        "the translated click must select the note"
    );
}

#[gpui::test]
fn plain_chars_insert_exactly_once(cx: &mut TestAppContext) {
    let (window, mut vcx) = open_editor(cx, |view| {
        view.component.set_clock("t".into(), 1);
        view.component.load_document(page_doc());
        add_textbox(view);
    });
    focus_editor(&window, &mut vcx);
    click_textbox(&mut vcx);

    // One 'b' must insert exactly one 'b' — the raw-key channel drops
    // bare printables, so only the InputHandler channel inserts.
    vcx.simulate_keystrokes("b");
    let text = window
        .update(&mut vcx, |view, _, _| textbox_content(view))
        .unwrap();
    assert_eq!(text, "Hellob", "plain char must insert exactly once");

    vcx.simulate_keystrokes("a");
    let text = window
        .update(&mut vcx, |view, _, _| textbox_content(view))
        .unwrap();
    assert_eq!(text, "Helloba");
}

#[gpui::test]
fn composition_mark_then_unmark_commits(cx: &mut TestAppContext) {
    use gpui::EntityInputHandler;

    let (window, mut vcx) = open_editor(cx, |view| {
        view.component.set_clock("t".into(), 1);
        view.component.load_document(page_doc());
        add_textbox(view);
    });
    focus_editor(&window, &mut vcx);
    click_textbox(&mut vcx);

    // Composition update: the shadow store marks, the kernel preedits —
    // the document content must not change yet.
    window
        .update(&mut vcx, |view, window, cx| {
            EntityInputHandler::replace_and_mark_text_in_range(view, None, "你", None, window, cx);
        })
        .unwrap();
    let text = window
        .update(&mut vcx, |view, _, _| textbox_content(view))
        .unwrap();
    assert_eq!(
        text, "Hello",
        "an active preedit must not touch the content"
    );

    // TSF confirm-as-is: unmark commits the marked string as-is.
    window
        .update(&mut vcx, |view, window, cx| {
            EntityInputHandler::unmark_text(view, window, cx);
        })
        .unwrap();
    let text = window
        .update(&mut vcx, |view, _, _| textbox_content(view))
        .unwrap();
    assert_eq!(text, "Hello你", "unmark must commit the marked text");
}

#[gpui::test]
fn ctrl_z_reverts_insert_and_shift_z_redoes(cx: &mut TestAppContext) {
    let (window, mut vcx) = open_editor(cx, |view| {
        view.component.set_clock("t".into(), 1);
        view.component.load_document(page_doc());
        add_textbox(view);
    });
    focus_editor(&window, &mut vcx);
    click_textbox(&mut vcx);
    vcx.simulate_keystrokes("b");
    assert_eq!(
        window
            .update(&mut vcx, |view, _, _| textbox_content(view))
            .unwrap(),
        "Hellob"
    );

    vcx.simulate_keystrokes("ctrl-z");
    assert_eq!(
        window
            .update(&mut vcx, |view, _, _| textbox_content(view))
            .unwrap(),
        "Hello",
        "ctrl+z must revert the insert"
    );

    vcx.simulate_keystrokes("ctrl-shift-z");
    assert_eq!(
        window
            .update(&mut vcx, |view, _, _| textbox_content(view))
            .unwrap(),
        "Hellob",
        "ctrl+shift+z must redo it"
    );
}

#[gpui::test]
fn ctrl_c_writes_body_selection_to_clipboard(cx: &mut TestAppContext) {
    let (window, mut vcx) = open_editor(cx, |view| {
        view.component.set_clock("t".into(), 1);
        view.component.load_document(body_text_doc());
    });
    focus_editor(&window, &mut vcx);

    // Drag over two text lines (the kernel-tested geometry: page points
    // (31, 25) -> (16, 45) select "CD\nE").
    let (page_x, page_y) = page_origin(WIN_W as f64);
    let at = |x: f64, y: f64| point(px((page_x + x) as f32), px((page_y + y) as f32));
    vcx.simulate_mouse_down(at(31.0, 25.0), MouseButton::Left, Modifiers::default());
    vcx.simulate_mouse_move(at(16.0, 45.0), MouseButton::Left, Modifiers::default());
    vcx.simulate_mouse_up(at(16.0, 45.0), MouseButton::Left, Modifiers::default());

    vcx.simulate_keystrokes("ctrl-c");
    let text = cx.read_from_clipboard().and_then(|item| item.text());
    assert_eq!(
        text.as_deref(),
        Some("CD\nE"),
        "ctrl+c must write the body selection to the clipboard"
    );
}

#[gpui::test]
fn ctrl_x_copies_without_deleting(cx: &mut TestAppContext) {
    // xilem parity (AGENTS Native Widget contract 5): Ctrl+X is
    // copy-only — a clipboard write, never a delete and never a history
    // push (the body is read-only and TextCursor has no extent).
    let (window, mut vcx) = open_editor(cx, |view| {
        view.component.set_clock("t".into(), 1);
        view.component.load_document(body_text_doc());
        // A TextBox placed OFF the drag path keeps the history non-empty,
        // so "can_undo unchanged" is a strong assertion.
        view.component.create_annotation(
            AnnotationKind::TextBox,
            PageId::new("P0"),
            AnnotationPayload::TextBox {
                rect: Rect {
                    x: 120.0,
                    y: 10.0,
                    w: 60.0,
                    h: 30.0,
                },
                content: "Hello".into(),
                font: FontId::new("F1"),
                size: 10.0,
                color: Color::Rgb(0, 0, 0),
                border: None,
                spans: Vec::new(),
            },
        );
    });
    focus_editor(&window, &mut vcx);

    // Same drag geometry as the ctrl+c test: page (31, 25) -> (16, 45).
    let (page_x, page_y) = page_origin(WIN_W as f64);
    let at = |x: f64, y: f64| point(px((page_x + x) as f32), px((page_y + y) as f32));
    vcx.simulate_mouse_down(at(31.0, 25.0), MouseButton::Left, Modifiers::default());
    vcx.simulate_mouse_move(at(16.0, 45.0), MouseButton::Left, Modifiers::default());
    vcx.simulate_mouse_up(at(16.0, 45.0), MouseButton::Left, Modifiers::default());

    let can_undo_before = window
        .update(&mut vcx, |view, _, _| view.component.can_undo())
        .unwrap();
    assert!(can_undo_before, "precondition: the history is non-empty");

    vcx.simulate_keystrokes("ctrl-x");
    let text = cx.read_from_clipboard().and_then(|item| item.text());
    assert_eq!(
        text.as_deref(),
        Some("CD\nE"),
        "ctrl+x must copy the body selection"
    );
    let can_undo_after = window
        .update(&mut vcx, |view, _, _| {
            assert_eq!(
                textbox_content(view),
                "Hello",
                "ctrl+x must not delete anything"
            );
            view.component.can_undo()
        })
        .unwrap();
    assert_eq!(
        can_undo_after, can_undo_before,
        "ctrl+x must not push a transaction"
    );
}
