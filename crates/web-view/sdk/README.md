# @office-rs/rofd

TypeScript SDK for embedding the [rofd](https://github.com/office-rs/rofd) OFD (GB/T 33190) **view + annotation** editor into any web page. Compiles the Rust editor to WebAssembly and renders via WebGPU, with automatic CPU soft-render fallback.

## Live Demo
[https://office-rs.github.io/rofd/](https://office-rs.github.io/rofd/)

## Features

- **View** OFD body (read-only render).
- **Annotate** — select body text and apply highlight / underline / strikeout / squiggly via `applyMarkup`, or draw with the freehand / rect / free-text tools.
- **Preserving save** — keeps the original package and rewrites only the annotation entries, byte for byte everywhere else; a signed document keeps its signatures.
- **Sign & verify** — verify the signatures of a file on opening (`verifySignatures`: one report per signature) or of the document that is open at any time (`ofd.verifySignatures`), sign the open document with an imported certificate file (`ofd.sign`: the view stays where it is; `signOfd` for a package outside an editor), and let the user place a seal with a click (`beginSealPlacement`).
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
| `loadOfd(bytes: Uint8Array)` | Load an `.ofd` package from raw bytes — another document: the view starts at the top, nothing of the previous one is kept. |
| `sign(pfx, password, options): Promise<SignatureReport[]>` | Sign the document that is open with a certificate file and its password. The signature goes into it: the seal shows on its page, the view / selection / undo history stay, and `saveOfd` carries it from here on. Nothing is written — that is the host's `saveOfd`. Resolves to the reports of every signature afterwards. |
| `verifySignatures(options?): Promise<SignatureReport[]>` | Verify the document that is open: what it carries now, saved or not — the signature `sign` just made included. No bytes to hand in, no file read. Verification never fails, it reports. |
| `isSignBlocked(): boolean` | The document's file declares signatures and has no annotation entry, and the document has annotations to write: `sign` would refuse it. Ask first, to tell the user why. |
| `prepareSign(certificateDer, options): PreparedSignature` | The two-phase form of `sign`, for a signer outside this process. |
| `saveOfd(): Uint8Array` | The current document as OFD package bytes — the preserving save: original entries byte for byte, only the annotation entries rewritten. Read the three flags below right after it. |
| `isSaveBlockedBySignature(): boolean` | The last `saveOfd()` could not write annotations: the file has no annotation entry and declares signatures. `saveOfd()` returned the file unchanged; ask the user, then `saveOfdFullWrite()`. |
| `isSaveReserveExhausted(): boolean` | The last save's annotation IDs went past the ID reserve written at signing time (they were written). |
| `isSaveFullWriteFallback(): boolean` | The last save fell back to the full write (an unsigned file had no anchor for a required patch). |
| `saveOfdFullWrite(): Uint8Array` | The full write — every modelled entry generated again. Invalidates the signatures of a file without an annotation entry; only after the user agreed. |
| `sealPictures(): (Uint8Array \| null)[]` | The seal picture of each signature of the document that is open, in the order `ofd.verifySignatures()` reports (`null` for a digital signature). |
| `beginSealPlacement(png, widthMm, heightMm): boolean` | Enter the seal placement state: the preview follows the pointer and the next left click on a page reports `onSealPlaced`. `false` when the picture cannot be decoded. |
| `cancelSealPlacement()` | Leave the state; `onSealPlacementCancelled` fires. |
| `setOnSealPlaced(cb)` / `setOnSealPlacementCancelled(cb)` | Handlers of the placement state (see below). |
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
| `onWarning(warnings: string[])` | Non-fatal issues: what the parser had to skip on a degraded load, and what a signature's landing has to tell (it comes through here too, like a load's). |
| `onAnnotationFocus(id)` | Annotation gains editing focus (e.g. double-click FreeText). |
| `onAnnotationInteract(id)` | Single-click interaction with an annotation (e.g. selecting a highlight). |
| `onPageChange(pageIndex)` | Page at viewport vertical center changed. |
| `onZoomChange(zoom)` | Zoom factor changed (1.0 = 100%). |
| `onSealPlaced(placement)` (via `setOnSealPlaced`) | The user placed a seal by clicking: `{ pageIndex, pageId, boundary: { x, y, w, h } }` (page millimetres). The state ended. |
| `onSealPlacementCancelled()` (via `setOnSealPlacementCancelled`) | The user cancelled the placement (Esc or the right mouse button). |

### Signing

The instance signs and verifies the document that is open (`ofd.sign`,
`ofd.verifySignatures`). The module functions are for a package that is not
in an editor, and for verification of one. The library never reads the
clock — pass the signing time.

```ts
import { Ofd, verifySignatures, newlyInvalidated, certificateInfo } from '@office-rs/rofd';

// On opening: one report per signature, [] when the file is not signed.
const reports = await verifySignatures(fileBytes, { now: '2026-11-24 10:30:00' });
// reports[i].overall: 'Valid' | 'Invalid' | 'Unverified' | 'Unparseable' | 'Unsupported'
// plus integrity, seal_match, signature_value, certificate, provider, date_time.
// integrity is { ok, failures, protected_files }: protected_files === 0 means
// the signature names no file — it protects nothing and reads 'Invalid'.

// A certificate file the user imported (kept by the app, still encrypted).
const entry = await certificateInfo(pfxBytes, password);   // throws on a wrong password

// A file that declares signatures and has no annotation entry cannot take the
// annotations in the editor: say so before trying (see the save flags above
// for the save's side of the same fact).
if (ofd.isSignBlocked()) {
  showError(new Error('the file has no annotation entry: settle the save first'));
}

// Sign: auto-stamped on the last page, or a digital signature (no `seal`).
const reportsAfter = await ofd.sign(pfxBytes, password, {
  seal: { png: sealPng, sizeMm: 40 },
  placement: { bottomRight: { pages: 'last', rightMm: 10, bottomMm: 10 } },
  now: '2026-11-24 10:30:00',
});

// The verify button, or any time: what the open document carries now - the
// signature just made included, before it is saved.
const current = await ofd.verifySignatures();
const stillValid = newlyInvalidated(reports, current);   // IDs that stopped being valid

// …or let the user place the seal (a modal state; Esc cancels):
ofd.setOnSealPlaced(({ pageIndex, boundary }) => {
  ofd.sign(pfxBytes, password, {
    seal: { png: sealPng, sizeMm: 40 },
    placement: { at: { page: pageIndex + 1, boundary } },
    now,
  }).catch((e: unknown) => showError(e));                 // never left floating
});
ofd.beginSealPlacement(sealPng, 40, 40);
```

`ofd.sign` puts the signature into the open document and makes no package:
nothing is loaded again, and `saveOfd` is an ordinary save that carries it.
Write what `saveOfd` gives when the user saves. `signOfd(ofd, pfx, password,
options)` is the same signature for a package that is not open — a file on
its way through a server: it returns the signed package. Do not `loadOfd`
that package into an editor that is already showing the document: it loads
another document and puts the view back at the first page.

`newlyInvalidated(before, after)` is for the pre-save check (M17): verify
the bytes a save would write with `verifySignatures(bytes)`, and the IDs it
names are the signatures that save takes from valid to anything else — ask
before writing.

A signer outside the page (a USB token, a remote service) uses
`ofd.prepareSign(certificateDer, options)` and the `toSign()` /
`apply(signature)` rounds of the `PreparedSignature` it returns — the
signature lands in the open document, as with `ofd.sign`. `apply` returns a
`SignStep`: `{ done: false, next, reports: null }` while there is a next
round, and `{ done: true, next: null, reports }` once the signature is in
the document — the same reports `ofd.sign` gives. Each `PreparedSignature`
is good for one `apply`, whether that answer was accepted, refused or
failed: wasm has taken the round, and the SDK says so if the object is used
again. Keep the `next` it hands back; after a refusal prepare again. For a
package outside an editor, `signPrepare(ofd, certificateDer, options)`
gives the same rounds and ends with the signed package (`PreparedSignOfd`,
`SignOfdStep`).

What a signature's landing has to warn about reaches the `onWarning`
handler given to `Ofd.init` — the same handler a load's warnings come
through. It is rare (a package made again for the signature, carrying an
element rofd does not model); what reading the file warned about at open is
not repeated when you sign.

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
