//! Floating annotation text-format panel state for the gpui adapter.
//!
//! The panel is built into [`crate::OfdGpuiView`]: the kernel provides the
//! target, style, anchor and font list; this module owns the UI state (which
//! dropdown is open) and the pure refresh logic, so it is headless-testable.

use rofd_component::{
    format_panel_anchor, AnnotationId, AnnotationSelection, AnnotationTextStyle, FamilyEntry,
    OfdComponent,
};

/// Panel footprint used for anchoring (viewport px).
pub(crate) const PANEL_W: f64 = 240.0;
pub(crate) const PANEL_H: f64 = 28.0;
/// Gap between the box and the panel.
pub(crate) const ANCHOR_MARGIN: f64 = 8.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PanelMenu {
    Family,
    Size,
    Color,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct FormatPanelState {
    pub target: Option<AnnotationId>,
    pub anchor: (f64, f64),
    pub style: Option<AnnotationTextStyle>,
    pub families: Vec<FamilyEntry>,
    pub open: Option<PanelMenu>,
}

impl FormatPanelState {
    /// Re-read the component: target = single-selected TextBox, plus its
    /// anchor and style. Clearing the selection hides the panel and closes
    /// any open dropdown.
    pub fn refresh(&mut self, component: &mut OfdComponent) {
        let target = match component.selection() {
            AnnotationSelection::Single(id) => Some(id.clone()),
            _ => None,
        };
        let Some(id) = target else {
            self.dismiss();
            return;
        };
        let Some(style) = component.annotation_text_style(&id) else {
            self.dismiss();
            return;
        };
        if let Some(rect) = component.annotation_format_anchor(&id) {
            let viewport = component.viewport_size();
            // Spec §1.1: hide when the box itself is not visible (scrolled or
            // paged away) - a panel pinned to an edge would edit an invisible
            // annotation.
            let visible = rect.x + rect.w > 0.0
                && rect.y + rect.h > 0.0
                && rect.x < viewport.0
                && rect.y < viewport.1;
            if !visible {
                self.dismiss();
                return;
            }
            self.anchor = format_panel_anchor(rect, (PANEL_W, PANEL_H), viewport, ANCHOR_MARGIN);
        }
        if self.target.as_ref() != Some(&id) || self.families.is_empty() {
            self.families = component.font_families();
        }
        self.target = Some(id);
        self.style = Some(style);
    }

    pub fn dismiss(&mut self) {
        self.target = None;
        self.style = None;
        self.open = None;
    }

    /// Whether the panel should be rendered.
    pub fn show(&self) -> bool {
        self.target.is_some() && self.style.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rofd_component::{AnnotationId, Modifiers, MouseButton, Tool, ViewEvent};
    use rofd_dom::{
        AnnotationKind, AnnotationPayload, Color, FontId, OfdDocument, Page, PageId, Rect,
    };
    use std::sync::Arc;

    fn component_with_textbox(viewport: (f64, f64), rect: Rect) -> (OfdComponent, AnnotationId) {
        let mut c = OfdComponent::new_native(rofd_component::OfdConfig::new(Arc::new(vec![])));
        c.set_clock("t".into(), 1);
        let mut doc = OfdDocument::default();
        doc.pages.push(Page {
            raw_children: Vec::new(),
            id: PageId::new("P0"),
            physical_box: Rect {
                x: 0.0,
                y: 0.0,
                w: 200.0,
                h: 200.0,
            },
            layers: vec![],
            template: None,
        });
        c.load_document(doc);
        c.set_viewport_size(viewport.0, viewport.1);
        c.set_tool(Tool::Text);
        let id = c.create_annotation(
            AnnotationKind::TextBox,
            PageId::new("P0"),
            AnnotationPayload::TextBox {
                rect,
                content: "hi".into(),
                font: FontId::new("F1"),
                size: 10.0,
                color: Color::Rgb(0, 0, 0),
                border: None,
                spans: vec![],
            },
        );
        // The component zooms by PX_PER_MM by default; mm -> viewport px.
        let zoom = c.zoom();
        click(&mut c, (rect.x + 10.0) * zoom, (rect.y + 10.0) * zoom);
        (c, id)
    }

    fn click(c: &mut OfdComponent, x: f64, y: f64) {
        c.handle_event(&ViewEvent::PointerDown {
            button: MouseButton::Left,
            x,
            y,
            modifiers: Modifiers::default(),
            click_count: 1,
        });
        c.handle_event(&ViewEvent::PointerUp {
            button: MouseButton::Left,
            x,
            y,
        });
    }

    #[test]
    fn refresh_targets_a_selected_textbox_and_clears_with_the_selection() {
        let (mut c, id) = component_with_textbox(
            (200.0, 200.0),
            Rect {
                x: 20.0,
                y: 20.0,
                w: 100.0,
                h: 40.0,
            },
        );
        let mut s = FormatPanelState::default();
        s.refresh(&mut c);
        assert_eq!(s.target.as_ref(), Some(&id));
        assert!(s.show());
        assert!(s.style.is_some());
        let zoom = c.zoom();
        click(&mut c, 180.0 * zoom, 180.0 * zoom); // empty page area -> deselect
        s.refresh(&mut c);
        assert!(s.target.is_none());
        assert!(!s.show());
        assert_eq!(s.open, None);
    }

    #[test]
    fn hides_when_the_box_scrolls_out_of_the_viewport() {
        let (mut c, _) = component_with_textbox(
            (200.0, 400.0),
            Rect {
                x: 20.0,
                y: 60.0,
                w: 100.0,
                h: 40.0,
            },
        );
        let mut s = FormatPanelState::default();
        s.refresh(&mut c);
        assert!(s.show());
        // Shrinking the viewport pushes the box below it (scroll clamps at 0).
        c.set_viewport_size(200.0, 100.0);
        s.refresh(&mut c);
        assert!(!s.show(), "box out of the viewport -> panel hides: {s:?}");
    }

    #[test]
    fn anchor_sits_above_and_flips_below_near_the_top() {
        let (mut c, id) = component_with_textbox(
            (200.0, 200.0),
            Rect {
                x: 20.0,
                y: 60.0,
                w: 100.0,
                h: 40.0,
            },
        );
        let box_rect = c.annotation_format_anchor(&id).unwrap();
        let mut s = FormatPanelState::default();
        s.refresh(&mut c);
        assert!(
            s.anchor.1 + PANEL_H <= box_rect.y,
            "panel above the box: {s:?} vs {box_rect:?}"
        );

        let (mut c, id) = component_with_textbox(
            (200.0, 200.0),
            Rect {
                x: 20.0,
                y: 2.0,
                w: 100.0,
                h: 40.0,
            },
        );
        let box_rect = c.annotation_format_anchor(&id).unwrap();
        let mut s = FormatPanelState::default();
        s.refresh(&mut c);
        assert!(
            s.anchor.1 > box_rect.y,
            "flipped below the box: {s:?} vs {box_rect:?}"
        );
    }
}
