//! The seal placement state of the component (spec 2026-11-23 §11).
//!
//! While it lasts, the component is modal: the next left click on a page
//! says where the seal goes, and nothing else can be selected, edited or
//! typed. Scrolling, turning pages and zooming stay available, and the seal
//! preview follows the pointer. The component only reports the position:
//! what is then signed is the host's business.

use std::sync::Arc;

use imaging::peniko::ImageData;
use imaging::record::Scene;
use imaging::Painter;
use rofd_dom::{PageId, Rect};

use crate::event::{EventOutcome, Key, MouseButton, ViewEvent};
use crate::ofd_component::{OfdComponent, Tool};
use crate::PointerCursor;

/// How opaque the preview is while it follows the pointer. It is baked into
/// the picture's alpha channel: the CPU fallback renderer refuses opacity on
/// an image draw.
pub const SEAL_PREVIEW_ALPHA: f32 = 0.6;

/// What the seal will look like: its picture and its size on the page.
#[derive(Debug, Clone, PartialEq)]
pub struct SealPreview {
    /// PNG (or JPEG) bytes.
    pub png: Arc<Vec<u8>>,
    pub width_mm: f64,
    pub height_mm: f64,
}

/// Where the user put the seal.
#[derive(Debug, Clone, PartialEq)]
pub struct SealPlacement {
    /// From 0. The host passes `page_index + 1` to the SDK.
    pub page_index: usize,
    pub page_id: PageId,
    /// Page millimetres: the `StampAnnot@Boundary` of the signature.
    pub boundary: Rect,
}

/// The placement state while it lasts.
pub(crate) struct SealPlacementState {
    preview: SealPreview,
    /// The decoded picture.
    image: ImageData,
    /// Where the pointer was last seen, in viewport pixels. `None` until it
    /// moved, and after a position that is not a number.
    pointer: Option<(f64, f64)>,
    /// The tool that was active before the state began.
    previous_tool: Tool,
}

impl OfdComponent {
    /// Enter the seal placement state: the next left click on a page reports
    /// where the seal goes ([`OfdComponent::on_seal_placed`]). Until then the
    /// component is modal - nothing can be selected, edited or typed - while
    /// scrolling, turning pages and zooming stay available.
    ///
    /// A state already in progress is replaced. Returns `false`, and does not
    /// enter, when the picture cannot be decoded or a size is not a positive
    /// number of millimetres.
    pub fn begin_seal_placement(&mut self, preview: SealPreview) -> bool {
        let Some(image) = preview_image(&preview) else {
            return false;
        };
        if !usable_size(preview.width_mm) || !usable_size(preview.height_mm) {
            return false;
        }
        // Text being composed belongs to the edit that came before.
        self.force_commit_preedit();
        // Whatever was dragged or hovered belongs to it too.
        self.drag = None;
        self.scrollbar_hover = None;
        self.hover = None;
        // A second begin keeps the tool the first one will bring back.
        let previous_tool = self
            .seal_placement
            .as_ref()
            .map_or_else(|| self.tool.clone(), |state| state.previous_tool.clone());
        self.seal_placement = Some(SealPlacementState {
            preview,
            image,
            pointer: None,
            previous_tool,
        });
        self.set_pointer_cursor(PointerCursor::Default);
        self.mark_scene_dirty();
        true
    }

    /// End the placement state as the user cancelling does:
    /// [`OfdComponent::on_seal_placement_cancelled`] is reported. Does
    /// nothing when no seal is being placed.
    pub fn cancel_seal_placement(&mut self) {
        if self.end_seal_placement() {
            self.fire_seal_placement_cancelled();
        }
    }

    pub fn seal_placement_active(&self) -> bool {
        self.seal_placement.is_some()
    }

    /// End the state without reporting anything: the document it was begun
    /// for is gone.
    pub(crate) fn drop_seal_placement(&mut self) {
        self.end_seal_placement();
    }

    /// Bring back the tool and the cursor from before the state. Returns
    /// whether a state was in progress.
    fn end_seal_placement(&mut self) -> bool {
        let Some(state) = self.seal_placement.take() else {
            return false;
        };
        self.tool = state.previous_tool;
        self.scrollbar_hover = None;
        self.set_tool_pointer_cursor();
        self.mark_scene_dirty();
        true
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn on_seal_placed(&mut self, cb: impl Fn(SealPlacement) + 'static + Send) {
        self.callbacks.on_seal_placed = Some(Box::new(cb));
    }
    #[cfg(target_arch = "wasm32")]
    pub fn on_seal_placed(&mut self, cb: impl Fn(SealPlacement) + 'static) {
        self.callbacks.on_seal_placed = Some(Box::new(cb));
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn on_seal_placement_cancelled(&mut self, cb: impl Fn() + 'static + Send) {
        self.callbacks.on_seal_placement_cancelled = Some(Box::new(cb));
    }
    #[cfg(target_arch = "wasm32")]
    pub fn on_seal_placement_cancelled(&mut self, cb: impl Fn() + 'static) {
        self.callbacks.on_seal_placement_cancelled = Some(Box::new(cb));
    }

    fn fire_seal_placed(&self, placement: &SealPlacement) {
        if let Some(cb) = &self.callbacks.on_seal_placed {
            cb(placement.clone());
        }
    }

    fn fire_seal_placement_cancelled(&self) {
        if let Some(cb) = &self.callbacks.on_seal_placement_cancelled {
            cb();
        }
    }

    /// An event while the placement state lasts. `None` for the events that
    /// go on as usual (scrolling, turning pages, zooming, resizing, focus);
    /// everything else is taken here, so no selection, edit or shortcut
    /// happens behind the seal.
    pub(crate) fn handle_seal_placement_event(
        &mut self,
        event: &ViewEvent,
    ) -> Option<EventOutcome> {
        match event {
            ViewEvent::PointerMove { x, y } => {
                // The scene changes only when the preview actually moves.
                let before = self.seal_preview_rect();
                self.note_seal_pointer((*x, *y));
                let moved = before != self.seal_preview_rect();
                if moved {
                    self.mark_scene_dirty();
                }
                Some(EventOutcome {
                    needs_repaint: moved,
                })
            }
            ViewEvent::PointerDown {
                button: MouseButton::Left,
                x,
                y,
                ..
            } => {
                // Off the pages nothing happens and the state goes on.
                if let Some(placement) = self.seal_placement_at((*x, *y)) {
                    self.end_seal_placement();
                    self.fire_seal_placed(&placement);
                    Some(EventOutcome {
                        needs_repaint: true,
                    })
                } else {
                    Some(EventOutcome {
                        needs_repaint: false,
                    })
                }
            }
            ViewEvent::PointerDown {
                button: MouseButton::Right,
                ..
            }
            | ViewEvent::KeyDown {
                key: Key::Escape, ..
            } => {
                self.cancel_seal_placement();
                Some(EventOutcome {
                    needs_repaint: true,
                })
            }
            ViewEvent::Scroll { .. }
            | ViewEvent::ScrollPage { .. }
            | ViewEvent::Zoom { .. }
            | ViewEvent::ZoomAt { .. }
            | ViewEvent::Resize { .. }
            | ViewEvent::FocusGained
            | ViewEvent::FocusLost => None,
            _ => Some(EventOutcome {
                needs_repaint: false,
            }),
        }
    }

    /// Follow the pointer. A tooltip would point at what is under the seal.
    fn note_seal_pointer(&mut self, pointer: (f64, f64)) {
        self.hover = None;
        if let Some(state) = self.seal_placement.as_mut() {
            state.pointer = (pointer.0.is_finite() && pointer.1.is_finite()).then_some(pointer);
        }
    }

    /// The page a viewport point lies on: its index and the point in page
    /// millimetres.
    fn page_at(&self, point: (f64, f64)) -> Option<(usize, (f64, f64))> {
        let doc = self.editor.document();
        let vp = self.viewport;
        rofd_render::page_origins(doc, &vp)
            .iter()
            .enumerate()
            .find_map(|(index, origin)| {
                let page = doc.pages.get(index)?;
                let (w, h) = (page.physical_box.w * vp.zoom, page.physical_box.h * vp.zoom);
                let inside = point.0 >= origin.0
                    && point.0 < origin.0 + w
                    && point.1 >= origin.1
                    && point.1 < origin.1 + h;
                inside.then(|| {
                    (
                        index,
                        (
                            (point.0 - origin.0) / vp.zoom,
                            (point.1 - origin.1) / vp.zoom,
                        ),
                    )
                })
            })
    }

    /// The seal on page `index`, centred on the page-millimetre point
    /// `local` and kept whole on the page.
    fn seal_boundary(&self, index: usize, local: (f64, f64)) -> Option<Rect> {
        let state = self.seal_placement.as_ref()?;
        let page = self.editor.document().pages.get(index)?;
        let (w, h) = (state.preview.width_mm, state.preview.height_mm);
        // To a thousandth of a millimetre: the boundary is written into the
        // signature as text. A seal larger than the page starts at the corner.
        Some(Rect {
            x: to_thousandth(local.0 - w / 2.0).clamp(0.0, (page.physical_box.w - w).max(0.0)),
            y: to_thousandth(local.1 - h / 2.0).clamp(0.0, (page.physical_box.h - h).max(0.0)),
            w,
            h,
        })
    }

    /// Where a click would put the seal. `None` off the pages.
    fn seal_placement_at(&self, point: (f64, f64)) -> Option<SealPlacement> {
        let (index, local) = self.page_at(point)?;
        Some(SealPlacement {
            page_index: index,
            page_id: self.editor.document().pages.get(index)?.id.clone(),
            boundary: self.seal_boundary(index, local)?,
        })
    }

    /// The preview as it is to be drawn now, in viewport pixels: `None`
    /// while the pointer is off the pages (or has not moved yet).
    pub(crate) fn seal_preview_rect(&self) -> Option<Rect> {
        let state = self.seal_placement.as_ref()?;
        let pointer = state.pointer?;
        let (index, local) = self.page_at(pointer)?;
        let boundary = self.seal_boundary(index, local)?;
        let (origin_x, origin_y) =
            rofd_render::page_origin(self.editor.document(), &self.viewport, index)?;
        let zoom = self.viewport.zoom;
        Some(Rect {
            x: origin_x + boundary.x * zoom,
            y: origin_y + boundary.y * zoom,
            w: boundary.w * zoom,
            h: boundary.h * zoom,
        })
    }

    /// The preview picture, for [`paint_seal_preview`].
    pub(crate) fn seal_preview_image(&self) -> Option<&ImageData> {
        self.seal_placement.as_ref().map(|state| &state.image)
    }
}

/// The preview picture as the renderer takes it: decoded to straight-alpha
/// Rgba8, with [`SEAL_PREVIEW_ALPHA`] multiplied into the alpha channel.
fn preview_image(preview: &SealPreview) -> Option<ImageData> {
    let image = rofd_render::decode_image(&preview.png)?;
    let mut pixels = image.data.data().to_vec();
    for pixel in pixels.as_chunks_mut::<4>().0 {
        pixel[3] = (f32::from(pixel[3]) * SEAL_PREVIEW_ALPHA).round() as u8;
    }
    Some(ImageData {
        data: imaging::peniko::Blob::from(pixels),
        format: image.format,
        alpha_type: image.alpha_type,
        width: image.width,
        height: image.height,
    })
}

fn usable_size(millimetres: f64) -> bool {
    millimetres.is_finite() && millimetres > 0.0
}

fn to_thousandth(millimetres: f64) -> f64 {
    (millimetres * 1000.0).round() / 1000.0
}

/// Draw the preview picture over `rect` (viewport pixels). The picture
/// carries the opacity (see [`preview_image`]); the draw itself is a plain
/// image draw, as the seals of a signed document are.
pub(crate) fn paint_seal_preview(overlay: &mut Scene, image: &ImageData, rect: Rect) {
    let (width, height) = (f64::from(image.width), f64::from(image.height));
    let scale_x = if width > 0.0 { rect.w / width } else { 1.0 };
    let scale_y = if height > 0.0 { rect.h / height } else { 1.0 };
    let place = imaging::kurbo::Affine::translate((rect.x, rect.y))
        * imaging::kurbo::Affine::scale_non_uniform(scale_x, scale_y);
    Painter::new(overlay).draw_image(image, place);
}

#[cfg(test)]
mod tests;
