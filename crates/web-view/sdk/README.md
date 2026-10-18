# @office-rs/rofd

TypeScript SDK for embedding the [rofd](https://github.com/office-rs/rofd) OFD (GB/T 33190) **view + annotation** editor into any web page. Compiles the Rust editor to WebAssembly and renders via WebGPU, with automatic CPU soft-render fallback.

## Live Demo
[https://office-rs.github.io/rofd/](https://office-rs.github.io/rofd/)

## Features

- **View** OFD body (read-only render).
- **Annotate** — select body text and apply highlight / underline / strikeout / squiggly via `applyMarkup`, or draw with the freehand / rect / free-text tools.
- **Surgical save** — re-serializes annotations and preserves untouched body entries byte-for-byte.
- **WebGPU 优先，CPU 自动回退** — WebGPU 不可用时自动切换 vello_cpu 软渲染（页位图缓存，滚动流畅）；可通过 `cpuFallback: false` 关闭。
- **Zero framework** — drop into any HTML page; the SDK owns the canvas, event binding, and render loop.
- **Persistent scrollbars** — classic vertical/horizontal scrollbars appear only when content overflows; drag the thumb, click the track to page, click the arrow buttons at the bar ends to step, or use the wheel. Scrolling is bounded at the first page top and last page bottom.

## Install

```bash
npm install @office-rs/rofd
# or: yarn add @office-rs/rofd / pnpm add @office-rs/rofd
```

## Browser support

WebGPU requires Chrome / Edge 113+ (or any browser shipping WebGPU). When WebGPU is unavailable, `Ofd.init` automatically falls back to CPU soft rendering (vello_cpu + page bitmap cache) and logs `[rofd] WebGPU 不可用，已切换 CPU 软渲染`; pass `cpuFallback: false` to opt out — then init throws on WebGPU-unsupported browsers.

## Quick start

```html
<div id="ofd-container" style="width: 800px; height: 600px"></div>

<script type="module">
  import { Ofd } from '@office-rs/rofd';

  const container = document.getElementById('ofd-container');
  const ofd = await Ofd.init(container, {
    // Ctrl+S handler — host decides where to persist the bytes.
    onSaveRequest: async () => {
      const bytes = ofd.saveOfd();
      await fetch('/api/save', { method: 'POST', body: bytes });
    },
  });

  // Author + timestamp must be injected before editing (the library never
  // reads the system clock — see AGENTS.md §4.4).
  ofd.setClock('ravenq', Date.now());

  // Load an .ofd package and start annotating.
  const resp = await fetch('/doc.ofd');
  ofd.loadOfd(new Uint8Array(await resp.arrayBuffer()));
</script>
```

## Ofd API

`Ofd.init(container, config?)` boots the editor: loads the wasm module, checks WebGPU, creates a canvas, initializes the wasm editor, loads fonts, wires callbacks, binds DOM events, and starts the render loop.

| Method | Description |
|---|---|
| `loadOfd(bytes: Uint8Array)` | Load an `.ofd` package from raw bytes. |
| `saveOfd(): Uint8Array` | Serialize the current document back to OFD package bytes. |
| `setClock(author: string, ts: number)` | Inject author + timestamp (ms) for subsequent edits. **Call before any annotation edit.** |
| `setTooltipEnabled(enabled: boolean)` | Toggle the hover tooltip (annotation author + creation time, local timezone). On by default. |
| `setTool(kind: string)` | Switch the active tool. One of: `select` (text), `hand`, `freehand`, `rect`, `ellipse`, `arrow`, `line`, `polygon`. Unknown values — including the legacy markup strings `highlight` / `underline` / `strikeout` / `squiggly` — fall back to the text tool. |
| `applyMarkup(kind: MarkupKind): string \| null` | Convert the current body-text selection into a markup annotation (`highlight` / `underline` / `strikeout` / `squiggly`; color via `setMarkupColor`). Returns the new annotation id, or `null` when there is no selection. The selection is kept so further markups can stack on the same range. |
| `hasTextSelection(): boolean` | Whether a body-text selection currently exists (drives markup-button enabled state). |
| `setOnTextSelectionChange(cb)` | Subscribe to the body-text selection appearing / changing / clearing. Signal-only — query `hasTextSelection()` / `getSelectedText()` afterwards. |
| `deleteAnnotation(id: string): boolean` | Delete the annotation with the given id. |
| `deleteSelected(): number` | Delete all currently-selected annotations; returns the count. |
| `handleScrollPage(direction: 'up' \| 'down')` | Scroll by one page height. |
| `handleZoomAt(factor: number, cx: number, cy: number)` | Zoom by `factor` while keeping `(cx, cy)` (device pixels) anchored. |
| `get canUndo` / `get canRedo` | Whether undo / redo is available. |
| `destroy()` | Stop rendering (dispose the scheduler) and remove the canvas. |

### Rendering

The SDK renders on demand: the render loop suspends when the editor has no
work (blurred or idle) and wakes on input, caret blink (~2 frames/s while
focused), or page visibility changes. A background tab is fully silenced -
zero frames, zero timers.

- `pauseRendering()` — host policy: stop rendering and drop pending work
  (e.g. a hidden panel or inactive tab in your app).
- `resumeRendering()` — resume after `pauseRendering()`; schedules one
  frame immediately.

`kick` / `withKick` / the scheduler are `@internal` and not part of the
public API surface.

## Callbacks (passed to `Ofd.init`)

| Callback | Fires when |
|---|---|
| `onChange` | Document changed (render loop re-renders). |
| `onSelectionChange` | Selection set changed. |
| `onTextSelectionChange` | Body-text selection appeared / changed / cleared. |
| `onCursorChange` | Caret / cursor moved. |
| `onSaveRequest` | Ctrl+S pressed — host should prompt for path or auto-save. |
| `onContextMenu(x, y, annotationId)` | Right-click. `annotationId` is `null` when hitting page body or desk background. |
| `onWarning(warnings: string[])` | Non-fatal parser issues (degraded load). |
| `onAnnotationFocus(id)` | Annotation gains editing focus (e.g. double-click FreeText). |
| `onAnnotationInteract(id)` | Single-click interaction with an annotation (e.g. selecting a highlight). |
| `onPageChange(pageIndex)` | Page at viewport vertical center changed. |
| `onZoomChange(zoom)` | Zoom factor changed (1.0 = 100%). |

## Fonts

The web can't read system fonts, and a bare `Ofd.init` now fetches **no fonts
at all** — the built-in jsDelivr Noto defaults (`DEFAULT_FONTS`) were removed.
Supply fonts explicitly through one of two channels:

- `fontSlices` — on-demand sliced channel: point `manifests` at slice
  manifests produced by the `@office-rs/font-runtime` toolchain; glyph subset
  slices load lazily as documents reference codepoints. `defaultFamily`
  prioritizes one family's slices; `includeDefaultNoto: true` also merges the
  built-in Noto npm-package manifests. Unreachable or missing manifests are
  dropped silently (degraded, non-blocking).
- `fonts` — whole font files, loaded eagerly.

```ts
await Ofd.init(container, {
  fontSlices: { manifests: ['/fonts/manifest.json'], defaultFamily: 'notosanscjksc' },
  // or whole fonts:
  // fonts: [{ url: '/fonts/MyFont.otf' }, { data: myFontBytes }],
});
```

## License

Apache-2.0. See [LICENSE](https://github.com/office-rs/rofd/blob/main/LICENSE).
