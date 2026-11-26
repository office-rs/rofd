# Rendering

> When to read: any task touching the render pipeline, Scene IR, dirty
> cache, text shaping, or the CPU fallback. Authoritative specs:
> `2026-10-13-web-render-on-demand`, `2026-10-15-no-gpu-support`.

## Scene IR (expansion of golden rule 5)

- `rofd-render` produces an **`imaging::record::Scene`** (the
  backend-agnostic drawing IR), **not** a `vello::Scene`; render only uses
  the imaging Painter API and never constructs a vello::Scene directly.
- Backend conversion lives in the adapters: web-view goes through
  imaging_vello (`VelloSceneSink`) to vello::Scene, the CPU fallback through
  imaging_vello_cpu; gpui-view replays through the gpui sink.

## Scene dirty cache (component's final public API)

`update_scene()` recomposes per the dirty flags, `scene() -> &Scene`,
`set_viewport_size(f64,f64)`, `mark_scene_dirty` (pub(crate)). Adapters call
`update_scene()` at paint time, then replay.

## Layered rendering

`page_body_scene` (`crates/render/src/layers.rs`) assembles the body scene
per OFD layer semantics; equivalence is pinned by
`crates/render/tests/layers_equivalence.rs`. Render tests use **scene
structure assertions** (object counts/types), never pixel comparison.

The seals of a signed document are **page content** (spec 2026-11-23 §8.3):
`draw_seals` (`crates/render/src/seal_scene.rs`) draws them into the cached
page scene after the body and before annotations, so they invalidate with
the page (and ride the CPU fallback's page bitmap cache); print and PDF
export gate them with `PrintContent.seal`. The seal **preview** of the
placement state is different — it follows the pointer, so it lives in the
per-frame overlay ([sign.md](sign.md)).

## Shaping determinism

TTC multi-family names anchor to the canonical name by the lowest face
index (see [fonts.md](fonts.md)); any new shaping path must preserve
"same input, same bytes across processes" — convert's parity test
(`crates/convert/tests/parity.rs`) depends on it.

## Web on-demand rendering (spec 2026-10-13)

The kernel's `needs_frame() -> FrameNeed` is the single source of truth; the
wasm fused export `renderFrame()` returns `0` (render again) / `>0` ms
(blink-deadline wake) / `-1` (fully suspended). The SDK-side scheduler and
kick mechanism: `crates/web-view/AGENTS.md`.

## WebGPU CPU fallback (spec 2026-10-15/10-18)

- When WebGPU is unavailable the SDK falls to `create_wasm_ofd_cpu`
  (vello_cpu + page bitmap cache) on **the same canvas**.
- **Ordering invariant**: `create_wasm_ofd` must probe the adapter
  (`compatible_surface: None`) BEFORE `create_surface` — once a canvas has a
  webgpu context bound it can **never** get a 2d context, and the fallback
  breaks. The chain is guarded by the `npm run smoke:nogpu` smoke test
  (headless Chrome --disable-gpu must land in the CPU fallback); do not
  revert to "bind the surface first, probe later".
- **vello_cpu refuses opacity on an image draw** ("not implemented:
  Applying opacity to image commands" — a panic). Anything translucent
  that is an image must carry the opacity in its pixels: the seal preview
  multiplies it into the picture's alpha channel. The web-app e2e
  `sign.spec.ts` paints that preview on the CPU path.

## v1 stubs (non-goals)

Template inheritance / JBIG2 / tiled images are on-demand, handled as v1
stubs (degradation paths emit `OfdWarning`, never silent).
