# @office-rs/rofd

TypeScript SDK for embedding the [rofd](https://github.com/office-rs/rofd) OFD (GB/T 33190) **view + annotation** editor into any web page. Compiles the Rust editor to WebAssembly and renders via WebGPU.

```ts
import { Editor } from '@office-rs/rofd';

const editor = await Editor.init(container, {
  onSaveRequest: () => download(editor.saveOfd()),
});
editor.loadOfd(ofdBytes);
```

## Features

- **View** OFD body (read-only render).
- **Annotate** with highlight / underline / strikeout / squiggly / freehand / rect / free-text.
- **Surgical save** — re-serializes annotations and preserves untouched body entries byte-for-byte.
- **WebGPU only** — no Canvas2D fallback. Chrome / Edge 113+ required.
- **Zero framework** — drop into any HTML page; the SDK owns the canvas, event binding, and render loop.

## Install

```bash
npm install @office-rs/rofd
# or: yarn add @office-rs/rofd / pnpm add @office-rs/rofd
```

## Browser support

WebGPU requires Chrome / Edge 113+ (or any browser shipping WebGPU). On unsupported browsers `Editor.init` throws `WebGPU is not supported in this browser`.

## Quick start

```html
<div id="ofd-container" style="width: 800px; height: 600px"></div>

<script type="module">
  import { Editor } from '@office-rs/rofd';

  const container = document.getElementById('ofd-container');
  const editor = await Editor.init(container, {
    // Ctrl+S handler — host decides where to persist the bytes.
    onSaveRequest: async () => {
      const bytes = editor.saveOfd();
      await fetch('/api/save', { method: 'POST', body: bytes });
    },
  });

  // Author + timestamp must be injected before editing (the library never
  // reads the system clock — see AGENTS.md §4.4).
  editor.setClock('ravenq', Date.now());

  // Load an .ofd package and start annotating.
  const resp = await fetch('/doc.ofd');
  editor.loadOfd(new Uint8Array(await resp.arrayBuffer()));
</script>
```

## Editor API

`Editor.init(container, config?)` boots the editor: loads the wasm module, checks WebGPU, creates a canvas, initializes the wasm editor, loads fonts, wires callbacks, binds DOM events, and starts the render loop.

| Method | Description |
|---|---|
| `loadOfd(bytes: Uint8Array)` | Load an `.ofd` package from raw bytes. |
| `saveOfd(): Uint8Array` | Serialize the current document back to OFD package bytes. |
| `setClock(author: string, ts: number)` | Inject author + timestamp (ms) for subsequent edits. **Call before any annotation edit.** |
| `setTool(kind: string)` | Switch the active tool. One of: `select`, `highlight`, `underline`, `strikeout`, `squiggly`, `freehand`, `rect`. Unknown values fall back to `select`. |
| `deleteAnnotation(id: string): boolean` | Delete the annotation with the given id. |
| `deleteSelected(): number` | Delete all currently-selected annotations; returns the count. |
| `handleScrollPage(direction: 'up' \| 'down')` | Scroll by one page height. |
| `handleZoomAt(factor: number, cx: number, cy: number)` | Zoom by `factor` while keeping `(cx, cy)` (device pixels) anchored. |
| `get canUndo` / `get canRedo` | Whether undo / redo is available. |
| `destroy()` | Stop the render loop and remove the canvas. |

## Callbacks (passed to `Editor.init`)

| Callback | Fires when |
|---|---|
| `onChange` | Document changed (render loop re-renders). |
| `onSelectionChange` | Selection set changed. |
| `onCursorChange` | Caret / cursor moved. |
| `onSaveRequest` | Ctrl+S pressed — host should prompt for path or auto-save. |
| `onContextMenu(x, y, annotationId)` | Right-click. `annotationId` is `null` when hitting page body or desk background. |
| `onWarning(warnings: string[])` | Non-fatal parser issues (degraded load). |
| `onAnnotationFocus(id)` | Annotation gains editing focus (e.g. double-click FreeText). |
| `onAnnotationInteract(id)` | Single-click interaction with an annotation (e.g. selecting a highlight). |
| `onPageChange(pageIndex)` | Page at viewport vertical center changed. |
| `onZoomChange(zoom)` | Zoom factor changed (1.0 = 100%). |

## Fonts

The web can't read system fonts, so the SDK fetches defaults from CDN (jsDelivr, ICP-licensed China CDN nodes):

- `NotoSans-Regular.ttf`
- `NotoSansCJKsc-Regular.otf`

To use custom fonts, pass `fonts` in `EditorConfig`:

```ts
await Editor.init(container, {
  fonts: [
    { url: '/fonts/MyFont.otf' },
    { data: myFontBytes }, // inline Uint8Array
  ],
});
```

## License

GPL-3.0-or-later. See [LICENSE](https://github.com/office-rs/rofd/blob/main/LICENSE).
