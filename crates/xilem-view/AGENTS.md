# rofd-xilem-view

> Auto-loaded when you work in this subtree. Fonts: docs/agents/fonts.md;
> rendering: docs/agents/rendering.md; annotation semantics:
> docs/agents/editing.md.

The thin masonry/xilem adapter embedding the editor as a first-class
masonry widget. **Depends only on component** (no io — io calls live in the
xilem-app host). The host never touches winit: focus, pointer capture, IME
sessions, and clipboard routing all go through masonry.

```
ofd(queue) / ofd_with_config(queue, config)    xilem View (ofd_view.rs)
  │ build: OfdWidget::new(config); actions ↔ .on_change/.on_context_menu/… handlers
  │ rebuild: drains `queue` → OfdWidget::with_component (host→widget commands)
  ▼
OfdWidget (masonry Widget)    owns OfdComponent, pending queue, effective focus,
  │                             pointer cursor
  │ masonry events → masonry_events.rs → ViewEvent
  │ component callbacks → pending queue → ctx.submit_action (after every touch)
  │ paint: component.update_scene() + painter.replay(scene)
  ▼
OfdComponent (rofd-component)
```

Ten key contracts:

1. **Host→widget commands**: `OfdCommand = Arc<dyn Fn(&mut OfdComponent) + Send + Sync>`;
   `OfdCommandQueue = Arc<Mutex<Vec<OfdCommand>>>`, drained via
   `with_component` at rebuild. Any handled callback returns
   `MessageResult::Action(())` → the host logic re-runs → rebuild → drain.
   Commands must be `Fn` (buttons re-fire); captured data is cloned inside
   the closure. A non-empty drain submits one internal
   `OfdWidgetAction::HostCommandWake` (mapped to no host handler; it only
   triggers one extra app_logic pass) so the host can consume command
   results; the next pass finds the queue empty and submits no further
   wake — bounded, no infinite re-render.
2. **Widget→host events**: `OfdWidgetAction` mirrors the component callback
   face; `PointerCursorChanged` is consumed internally (drives
   `get_cursor`) and never submitted.
3. **Effective focus** = widget keyboard focus ∧ window focus; the component
   is born unfocused (the caret stays hidden before the first click).
   `Ime::Disabled → FocusLost` force-commits preedit, with a defensive
   regain.
4. **IME**: `accepts_text_input` starts/stops the session automatically;
   every touch ends by refreshing `set_ime_area` from `caret_rect()`.
5. **Clipboard**: Ctrl+V = `TextEvent::ClipboardPaste` → `paste_text`;
   Ctrl+C is intercepted in the widget and written out via
   `ctx.set_clipboard`. Ctrl+X is copy-only in rofd (the body is read-only
   at runtime, no selection extent — never deletes); host menus have no Cut
   item.
6. **Zoom (a deliberate rofd difference, no mirror)**: the component
   viewport scales multiplicatively (`zoom *= factor`, baseline
   `PX_PER_MM = 96/25.4`); ctrl+wheel sends `ZoomAt{factor, center}`
   (×1.1 / ×0.9, center = the pointer's viewport position), clamped to
   `[PX_PER_MM×0.25, PX_PER_MM×3.0]`. Host/widget keep no f32 zoom mirror.
   Fit-zoom modes (fit width/height/page, spec 2026-10-10):
   `set_zoom_mode(ZoomMode)`, the baseline page = the current page;
   resize/document switch re-fits automatically, page turns do not; manual
   zoom exits to Custom and fires `on_zoom_mode_change`. Fit results clamp
   to `[FIT_MIN_ZOOM, MAX_ZOOM]` (5%~300%); the manual lower bound is
   dynamic — once zoom is below 25%, the bound loosens to 5% (prevents
   upward drag).
7. **Blink**: `on_anim_frame` keeps the animation alive while focused; the
   500ms timing belongs to the component's `tick_blink` (monotonic
   Instant).
8. **Scroll**: wheel events are set_handled — scrolling and the
   self-drawn scrollbars belong to the component; no portal, no
   CANVAS_CONTENT_HEIGHT.
9. **Accessibility**: `Role::Document`.
10. **Seal placement** (spec 2026-11-23 §11): entering and cancelling are
   host commands (`component.begin_seal_placement(..)` /
   `cancel_seal_placement()` through the queue — no new API); the click and
   the cancellation come back as `OfdWidgetAction::SealPlaced(SealPlacement)`
   / `SealPlacementCancelled`, wired by `.on_seal_placed(..)` /
   `.on_seal_placement_cancelled(..)`. The adapter does not depend on
   `rofd-sign`: the host signs (docs/agents/sign.md).

If you are tempted to reintroduce a winit-layer bridge, canvas-origin
bookkeeping, or a per-frame force_repaint ordering — **don't**; the widget
is the entire platform adapter.

Tests: the masonry_testing headless harness, 10 cases (list in
docs/agents/workflow.md §test conventions).

Pending: the annotation format-panel UI (spec 2026-11-10 §5.3; gpui-view
and the web SDK are done).
