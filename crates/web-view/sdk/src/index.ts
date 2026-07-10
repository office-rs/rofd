// @rofd/sdk - TypeScript SDK wrapping the rofd WASM editor exports.
//
// Wraps the generated `WasmEditor` class (from wasm-pack) in an idiomatic
// `Editor` class with a static async factory (`Editor.create`). The web-app
// (Task 5) consumes this SDK via `import { Editor } from '@rofd/sdk'`.

import init, { WasmEditor } from '../dist/rofd_web_view.js';

/**
 * Idiomatic TypeScript wrapper around the raw `WasmEditor` wasm export.
 *
 * Usage:
 * ```ts
 * const editor = await Editor.create(canvas, fontBytes);
 * editor.loadOfd(ofdBytes);
 * editor.render();
 * ```
 *
 * The wasm module is initialised lazily on first `create` call via the
 * `init` default export (wasm-pack `--target web` pattern).
 */
export class Editor {
  private readonly editor: WasmEditor;

  private constructor(editor: WasmEditor) {
    this.editor = editor;
  }

  /**
   * Create a new editor bound to a canvas element.
   *
   * Initialises the wasm module (if not already initialised) and constructs
   * the `WasmEditor` (which requests a WebGPU adapter + device for the
   * canvas). Async because WebGPU device acquisition is async in the browser.
   *
   * @param canvas     The canvas element to render into.
   * @param fontBytes  Document font bytes (empty for v1 - text won't render).
   * @returns A ready-to-use `Editor` instance.
   */
  static async create(canvas: HTMLCanvasElement, fontBytes: Uint8Array): Promise<Editor> {
    await init();
    // wasm-bindgen exports the async constructor as `new WasmEditor(...)`.
    // The constructor returns a Promise because WebGpuRenderTarget::new is
    // async (wasm-bindgen async fn -> JS Promise).
    const editor = await new WasmEditor(canvas, fontBytes);
    return new Editor(editor);
  }

  // ─── File I/O ───────────────────────────────────────────────────────────────

  /**
   * Load an OFD document from raw `.ofd` package bytes.
   * Replaces any previously loaded document.
   */
  loadOfd(bytes: Uint8Array): void {
    this.editor.load_ofd(bytes);
  }

  /**
   * Serialize the current document to OFD package bytes.
   * @returns A `Uint8Array` that can be wrapped in a `Blob` for download.
   */
  saveOfd(): Uint8Array {
    return this.editor.save_ofd();
  }

  // ─── Input handling ────────────────────────────────────────────────────────

  /**
   * Handle a keydown event.
   * @param key   `KeyboardEvent.key` (e.g. `"Enter"`, `"a"`, `"ArrowLeft"`).
   * @param ctrl  Whether Ctrl (or Meta) was held.
   * @param shift Whether Shift was held.
   * @returns `true` if the editor needs a repaint.
   */
  handleKeydown(key: string, ctrl: boolean, shift: boolean): boolean {
    return this.editor.handle_keydown(key, ctrl, shift);
  }

  /**
   * Handle a pointerdown event.
   * @param x       Canvas-relative X (CSS pixels).
   * @param y       Canvas-relative Y (CSS pixels).
   * @param button  `MouseEvent.button` (0=left, 1=middle, 2=right).
   * @returns `true` if a repaint is needed.
   */
  handlePointerDown(x: number, y: number, button: number): boolean {
    return this.editor.handle_pointerdown(x, y, button);
  }

  /**
   * Handle a pointerup event.
   * @returns `true` if a repaint is needed.
   */
  handlePointerUp(x: number, y: number, button: number): boolean {
    return this.editor.handle_pointerup(x, y, button);
  }

  /**
   * Handle a pointermove event.
   * @returns `true` if a repaint is needed.
   */
  handlePointerMove(x: number, y: number): boolean {
    return this.editor.handle_pointermove(x, y);
  }

  /**
   * Handle a scroll/wheel event.
   * @param dx  Horizontal delta (CSS pixels, from `WheelEvent.deltaX`).
   * @param dy  Vertical delta (CSS pixels, from `WheelEvent.deltaY`).
   * @returns `true` if a repaint is needed.
   */
  handleScroll(dx: number, dy: number): boolean {
    return this.editor.handle_scroll(dx, dy);
  }

  /**
   * Handle a canvas resize. Reconfigures the WebGPU surface and updates
   * the editor viewport.
   * @returns `true` if a repaint is needed.
   */
  handleResize(width: number, height: number): boolean {
    return this.editor.handle_resize(width, height);
  }

  // ─── Rendering ──────────────────────────────────────────────────────────────

  /**
   * Render the current editor state to the canvas via WebGPU + vello.
   * Call this from a `requestAnimationFrame` loop whenever any `handle*`
   * method returned `true`.
   */
  render(): void {
    this.editor.render();
  }

  // ─── Undo / redo ───────────────────────────────────────────────────────────

  /** Whether there are undoable operations in the history. */
  get canUndo(): boolean {
    return this.editor.can_undo();
  }

  /** Whether there are redoable operations in the history. */
  get canRedo(): boolean {
    return this.editor.can_redo();
  }

  // ─── Annotation clock ──────────────────────────────────────────────────────

  /**
   * Set the annotation clock (author + timestamp) for subsequent edits.
   * @param author  Author identifier for new annotations.
   * @param ts      Unix timestamp (milliseconds) for new annotations.
   */
  setClock(author: string, ts: number): void {
    // i64 maps to bigint in wasm-bindgen; convert from JS number.
    this.editor.set_clock(author, BigInt(ts));
  }
}
