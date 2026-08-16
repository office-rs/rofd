// @rofd/sdk - TypeScript SDK wrapping the rofd WASM editor.
//
// Mirrors reditor's SDK: `Editor.init(container, config)` does the full boot
// (load wasm -> check WebGPU -> create canvas -> create_wasm_editor -> load +
// register fonts -> register callbacks -> bindEvents -> render loop). The web
// app just calls `init` and optionally passes fonts/callbacks.

// --- wasm module shape (wasm-pack --target web) ---
// `default` is the async init; `create_wasm_editor` is the factory.
type WasmModule = {
  default(): Promise<void>;
  create_wasm_editor(canvas: HTMLCanvasElement): Promise<WasmEditor>;
};

interface WasmEditor {
  renderFrame(): void;
  registerFont(bytes: Uint8Array): boolean;
  handleResize(width: number, height: number): void;
  handleKeyDown(
    key: string,
    shift: boolean,
    ctrl: boolean,
    alt: boolean,
    meta: boolean,
  ): void;
  handleMouseDown(
    button: number,
    x: number,
    y: number,
    shift: boolean,
    ctrl: boolean,
    alt: boolean,
    meta: boolean,
  ): void;
  handleMouseUp(
    button: number,
    x: number,
    y: number,
    shift: boolean,
    ctrl: boolean,
    alt: boolean,
    meta: boolean,
  ): void;
  handleMouseMove(x: number, y: number): void;
  handleMouseScroll(dx: number, dy: number): void;
  handleZoom(factor: number): void;
  handleScrollPage(direction: 'up' | 'down'): void;
  handleZoomAt(factor: number, cx: number, cy: number): void;
  handleFocusGained(): void;
  handleFocusLost(): void;
  loadOfd(bytes: Uint8Array): void;
  saveOfd(): Uint8Array;
  canUndo(): boolean;
  canRedo(): boolean;
  setClock(author: string, ts: bigint): void;
  setOnChange(cb: (() => void) | null): void;
  setOnSelectionChange(cb: (() => void) | null): void;
  setOnCursorChange(cb: (() => void) | null): void;
  setOnSaveRequest(cb: (() => void) | null): void;
  setOnContextMenu(cb: ((x: number, y: number, annotationId: string | null) => void) | null): void;
  setOnWarning(cb: ((warnings: string[]) => void) | null): void;
  setOnAnnotationFocus(cb: ((annotationId: string) => void) | null): void;
  setOnAnnotationInteract(cb: ((annotationId: string) => void) | null): void;
  setOnPageChange(cb: ((pageIndex: number) => void) | null): void;
  setOnZoomChange(cb: ((zoom: number) => void) | null): void;
  setOnPointerCursor(cb: ((shape: string) => void) | null): void;
  setTool(kind: string): void;
  deleteAnnotation(id: string): boolean;
  deleteSelected(): number;
}

// --- public SDK types ---

/** A font source: either a URL to fetch or inline bytes. */
export interface FontSource {
  url?: string;
  data?: Uint8Array;
}

/** Configuration for `Editor.init`. */
export interface EditorConfig {
  /** Fonts to load + register. Defaults to Noto Sans + Noto Sans CJK SC from CDN. */
  fonts?: FontSource[];
  /** Fired when the document changes (signal-only; the render loop re-renders). */
  onChange?: () => void;
  onSelectionChange?: () => void;
  onCursorChange?: () => void;
  /** Fired on Ctrl+S (the host should prompt for a path / trigger save). */
  onSaveRequest?: () => void;
  /** Fired on right-click. `annotationId` is null when the click hit a page
   * body or the desk background (no annotation to act on). */
  onContextMenu?: (x: number, y: number, annotationId: string | null) => void;
  /** Fired after `loadOfd` when the parser encountered non-fatal issues
   * (degraded load). Receives an array of human-readable warning strings. */
  onWarning?: (warnings: string[]) => void;
  /** Fired when an annotation gains editing focus (e.g. double-click a
   * FreeText to enter text-edit mode). Receives the annotation id. */
  onAnnotationFocus?: (annotationId: string) => void;
  /** Fired on a single-click interaction with an annotation (e.g. selecting
   * a highlight). Receives the annotation id. */
  onAnnotationInteract?: (annotationId: string) => void;
  /** Fired when the page at the viewport's vertical center changes (scrolling
   * or zooming past a page boundary). Receives the 0-based page index. */
  onPageChange?: (pageIndex: number) => void;
  /** Fired when the viewport zoom changes. Receives the new zoom factor
   * (1.0 = 100%). Only fires when the zoom actually differs. */
  onZoomChange?: (zoom: number) => void;
}

// Default font CDN (jsDelivr - ICP-licensed China CDN nodes). Same fonts reditor
// uses. The web can't access system fonts, so these are the only font source.
const FONT_CDN_BASE =
  'https://cdn.jsdelivr.net/gh/googlefonts/noto-cjk@main/Sans/OTF/SimplifiedChinese';
const DEFAULT_FONTS: FontSource[] = [
  { url: `${FONT_CDN_BASE}/NotoSans-Regular.ttf` },
  { url: `${FONT_CDN_BASE}/NotoSansCJKsc-Regular.otf` },
];

/**
 * rofd web editor. Created via [`Editor.init`]; the SDK owns the canvas, the
 * wasm editor, DOM event binding, and the render loop.
 *
 * Usage:
 * ```ts
 * const editor = await Editor.init(container, {
 *   onSaveRequest: () => download(editor.saveOfd()),
 * });
 * editor.loadOfd(ofdBytes);
 * ```
 */
export class Editor {
  private wasm: WasmEditor;
  private canvas: HTMLCanvasElement;
  private animFrameId: number | null = null;
  private abortController: AbortController;

  private constructor(wasm: WasmEditor, canvas: HTMLCanvasElement) {
    this.wasm = wasm;
    this.canvas = canvas;
    this.abortController = new AbortController();
  }

  /**
   * Initialize a new editor inside `container`: loads the wasm module, checks
   * WebGPU, creates a canvas, initializes the wasm editor (WebGPU + warmup),
   * loads + registers fonts, wires callbacks, binds DOM events, and starts the
   * render loop. Returns a ready-to-use `Editor`.
   */
  static async init(
    container: HTMLElement,
    config?: EditorConfig,
  ): Promise<Editor> {
    // 1. Load wasm module (--target web: fetch-based, auto-resolves .wasm).
    const wasm = (await import('../dist/rofd_web_view.js')) as unknown as WasmModule;
    await wasm.default();

    // 2. Check WebGPU support.
    if (!navigator.gpu) {
      throw new Error('WebGPU is not supported in this browser');
    }

    // 3. Create canvas inside container.
    const canvas = document.createElement('canvas');
    canvas.tabIndex = 0;
    canvas.style.width = '100%';
    canvas.style.height = '100%';
    canvas.style.outline = 'none';
    // Initial cursor matches the component's PointerCursor::Default state;
    // the onPointerCursor callback takes over on the first state change.
    canvas.style.cursor = 'default';
    container.appendChild(canvas);

    // 4. Create WasmEditor (async: WebGPU init + warmup).
    const wasmEditor = await wasm.create_wasm_editor(canvas);

    // 5. Load + register fonts (web can't access system fonts).
    const fonts = config?.fonts ?? DEFAULT_FONTS;
    for (const font of fonts) {
      try {
        const bytes = await loadFont(font);
        wasmEditor.registerFont(bytes);
      } catch (e) {
        console.warn('[rofd] font load failed; text may not render', e);
      }
    }

    // 6. Register callbacks.
    if (config?.onChange) wasmEditor.setOnChange(config.onChange);
    if (config?.onSelectionChange) wasmEditor.setOnSelectionChange(config.onSelectionChange);
    if (config?.onCursorChange) wasmEditor.setOnCursorChange(config.onCursorChange);
    if (config?.onSaveRequest) wasmEditor.setOnSaveRequest(config.onSaveRequest);
    if (config?.onContextMenu) wasmEditor.setOnContextMenu(config.onContextMenu);
    if (config?.onWarning) wasmEditor.setOnWarning(config.onWarning);
    if (config?.onAnnotationFocus) wasmEditor.setOnAnnotationFocus(config.onAnnotationFocus);
    if (config?.onAnnotationInteract) wasmEditor.setOnAnnotationInteract(config.onAnnotationInteract);
    if (config?.onPageChange) wasmEditor.setOnPageChange(config.onPageChange);
    if (config?.onZoomChange) wasmEditor.setOnZoomChange(config.onZoomChange);

    // Pointer cursor: the wasm side reports CSS cursor names directly
    // ("default"/"grab"/"grabbing"), so no mapping is needed here.
    wasmEditor.setOnPointerCursor((shape: string) => {
      canvas.style.cursor = shape;
    });

    // 7. Create wrapper + bind DOM events.
    const editor = new Editor(wasmEditor, canvas);
    editor.bindEvents();

    // Prevent the browser's native context menu on the canvas (right-click).
    canvas.addEventListener('contextmenu', (e) => e.preventDefault());

    // 8. Initial resize + render loop.
    editor.resize();
    editor.startRenderLoop();

    // 9. Focus the canvas so keyboard input works immediately.
    canvas.focus();

    return editor;
  }

  /** Destroy the editor: stop the render loop, remove canvas, abort listeners. */
  destroy(): void {
    if (this.animFrameId !== null) {
      cancelAnimationFrame(this.animFrameId);
      this.animFrameId = null;
    }
    this.abortController.abort();
    if (this.canvas.parentNode) {
      this.canvas.parentNode.removeChild(this.canvas);
    }
  }

  // ─── Internal: event binding ──────────────────────────────────────────────

  /** Bind DOM events on the canvas, translating them to wasm calls. */
  private bindEvents(): void {
    const opts: AddEventListenerOptions = { signal: this.abortController.signal };
    const dpr = () => window.devicePixelRatio || 1;

    // Keyboard.
    this.canvas.addEventListener(
      'keydown',
      (e: KeyboardEvent) => {
        e.preventDefault();
        // PageUp/PageDown scroll by one page height (ScrollPage), not a
        // generic KeyDown: the component's handle_key doesn't act on these
        // keys, so routing them as ScrollPage gives them an effect (viewport
        // scroll by page_h + page_gap). Mirrors the native winit bridge.
        if (e.key === 'PageUp') {
          this.wasm.handleScrollPage('up');
        } else if (e.key === 'PageDown') {
          this.wasm.handleScrollPage('down');
        } else {
          this.wasm.handleKeyDown(e.key, e.shiftKey, e.ctrlKey, e.altKey, e.metaKey);
        }
      },
      opts,
    );

    // Pointer events (coords in device pixels: CSS * DPR). pointerdown
    // captures the pointer so pointermove/pointerup keep firing on the
    // canvas even when the drag is released outside it (e.g. over the
    // ribbon); without capture the component would never see the up and
    // the drag state (hand-tool pan, grabbing cursor) would stick.
    this.canvas.addEventListener(
      'pointerdown',
      (e: PointerEvent) => {
        e.preventDefault();
        this.canvas.focus();
        this.canvas.setPointerCapture(e.pointerId);
        const rect = this.canvas.getBoundingClientRect();
        this.wasm.handleMouseDown(
          e.button,
          (e.clientX - rect.left) * dpr(),
          (e.clientY - rect.top) * dpr(),
          e.shiftKey,
          e.ctrlKey,
          e.altKey,
          e.metaKey,
        );
      },
      opts,
    );

    this.canvas.addEventListener(
      'pointerup',
      (e: PointerEvent) => {
        const rect = this.canvas.getBoundingClientRect();
        this.wasm.handleMouseUp(
          e.button,
          (e.clientX - rect.left) * dpr(),
          (e.clientY - rect.top) * dpr(),
          e.shiftKey,
          e.ctrlKey,
          e.altKey,
          e.metaKey,
        );
      },
      opts,
    );

    // Fallback: the browser cancels the pointer (e.g. touch gesture
    // takeover); forward as an up so any drag state still clears.
    this.canvas.addEventListener(
      'pointercancel',
      (e: PointerEvent) => {
        const rect = this.canvas.getBoundingClientRect();
        this.wasm.handleMouseUp(
          e.button,
          (e.clientX - rect.left) * dpr(),
          (e.clientY - rect.top) * dpr(),
          e.shiftKey,
          e.ctrlKey,
          e.altKey,
          e.metaKey,
        );
      },
      opts,
    );

    this.canvas.addEventListener(
      'pointermove',
      (e: PointerEvent) => {
        const rect = this.canvas.getBoundingClientRect();
        this.wasm.handleMouseMove((e.clientX - rect.left) * dpr(), (e.clientY - rect.top) * dpr());
      },
      opts,
    );

    // Scroll / zoom.
    this.canvas.addEventListener(
      'wheel',
      (e: WheelEvent) => {
        e.preventDefault();
        if (e.ctrlKey || e.metaKey) {
          // Ctrl+wheel: zoom anchored to the cursor position (device px).
          const rect = this.canvas.getBoundingClientRect();
          const cx = (e.clientX - rect.left) * dpr();
          const cy = (e.clientY - rect.top) * dpr();
          this.wasm.handleZoomAt(e.deltaY > 0 ? 0.9 : 1.1, cx, cy);
        } else {
          this.wasm.handleMouseScroll(e.deltaX, e.deltaY);
        }
      },
      { ...opts, passive: false },
    );

    // Focus.
    this.canvas.addEventListener('focus', () => this.wasm.handleFocusGained(), opts);
    this.canvas.addEventListener('blur', () => this.wasm.handleFocusLost(), opts);

    // Resize.
    window.addEventListener('resize', () => this.resize(), opts);
  }

  /** Resize the canvas backing store to match its CSS size (× DPR) + notify wasm. */
  private resize(): void {
    const rect = this.canvas.getBoundingClientRect();
    const dpr = window.devicePixelRatio || 1;
    const width = Math.max(1, Math.round(rect.width * dpr));
    const height = Math.max(1, Math.round(rect.height * dpr));
    this.canvas.width = width;
    this.canvas.height = height;
    this.wasm.handleResize(width, height);
    this.wasm.renderFrame();
  }

  /** Start the requestAnimationFrame render loop. */
  private startRenderLoop(): void {
    const loop = () => {
      this.wasm.renderFrame();
      this.animFrameId = requestAnimationFrame(loop);
    };
    this.animFrameId = requestAnimationFrame(loop);
  }

  // ─── Public API ───────────────────────────────────────────────────────────

  /** Load an OFD document from raw `.ofd` package bytes. */
  loadOfd(bytes: Uint8Array): void {
    this.wasm.loadOfd(bytes);
  }

  /** Serialize the current document to OFD package bytes. */
  saveOfd(): Uint8Array {
    return this.wasm.saveOfd();
  }

  /** Whether there are undoable operations. */
  get canUndo(): boolean {
    return this.wasm.canUndo();
  }

  /** Whether there are redoable operations. */
  get canRedo(): boolean {
    return this.wasm.canRedo();
  }

  /** Set the annotation clock (author + timestamp ms) for subsequent edits. */
  setClock(author: string, ts: number): void {
    // i64 maps to BigInt in wasm-bindgen; convert from JS number.
    this.wasm.setClock(author, BigInt(ts));
  }

  /** Set the active editing tool. `kind` is one of: "select", "highlight",
   * "underline", "strikeout", "squiggly", "freehand", "rect". Unknown values
   * fall back to "select". */
  setTool(kind: string): void {
    this.wasm.setTool(kind);
  }

  /** Delete the annotation with the given id string. Returns false if no
   * annotation with that id exists. */
  deleteAnnotation(id: string): boolean {
    return this.wasm.deleteAnnotation(id);
  }

  /** Delete all currently-selected annotations. Returns the count deleted. */
  deleteSelected(): number {
    return this.wasm.deleteSelected();
  }

  /** Scroll by one page height. `direction` is "up" or "down". Intended for
   * PageUp/PageDown keys. */
  handleScrollPage(direction: 'up' | 'down'): void {
    this.wasm.handleScrollPage(direction);
  }

  /** Zoom by `factor` while keeping the `(cx, cy)` viewport point (device
   * pixels) anchored to the same document position. Intended for Ctrl+wheel
   * zoom (cursor position = center). */
  handleZoomAt(factor: number, cx: number, cy: number): void {
    this.wasm.handleZoomAt(factor, cx, cy);
  }
}

// ─── Font loading ─────────────────────────────────────────────────────────────

/** Load a font: inline `data` if present, else `fetch(url)`. */
async function loadFont(source: FontSource): Promise<Uint8Array> {
  if (source.data) return source.data;
  if (!source.url) throw new Error('FontSource must have url or data');
  const response = await fetch(source.url);
  if (!response.ok) throw new Error(`Failed to load font: ${source.url}`);
  const buffer = await response.arrayBuffer();
  return new Uint8Array(buffer);
}
