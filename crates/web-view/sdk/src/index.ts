// @office-rs/rofd - TypeScript SDK wrapping the rofd WASM OFD surface.
//
// Mirrors the as-built adapter design: `Ofd.init(container, config)` does the
// full boot (load wasm -> check WebGPU -> create canvas -> create_wasm_ofd ->
// fonts [legacy whole fonts awaited, or the slice flow started after wire] ->
// register callbacks -> bindEvents -> start on-demand rendering).
// The web app just calls `init` and optionally passes fonts/callbacks.

import { FontRuntime } from '@office-rs/font-runtime';
import { RenderScheduler } from './scheduler';
import { withKick } from './with-kick';
import * as fontFlow from './fontFlow';
import type { FontSlicesConfig } from './fontFlow';

export type { FontSlicesConfig } from './fontFlow';

// --- wasm module shape (wasm-pack --target web) ---
// `default` is the async init; `create_wasm_ofd` is the factory.
type WasmModule = {
  default(): Promise<void>;
  create_wasm_ofd(canvas: HTMLCanvasElement): Promise<WasmOfd>;
};

interface WasmOfd {
  /**
   * Render one frame and return the next scheduling need:
   * `0` = render again immediately, `>0` = ms until the next needed
   * frame (blink deadline), `-1` = suspend until an event kick.
   */
  renderFrame(): number;
  registerFont(bytes: Uint8Array): boolean;
  /** Current font requirements: normalized family keys + sorted deduped codepoints. */
  collectFontRequirements(): { families: string[]; codepoints: number[] };
  /** Register a decoded (OTF/TTF) font slice. `false` = bytes parsed to no family. */
  registerFontSlice(bytes: Uint8Array, familyKey: string, sliceId: string): boolean;
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
    clickCount: number,
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
  undo(): boolean;
  redo(): boolean;
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
  setZoomMode(mode: string): void;
  zoomMode(): string;
  setOnZoomModeChange(cb: ((mode: string) => void) | null): void;
  setOnPointerCursor(cb: ((shape: string) => void) | null): void;
  setOnCopy(cb: ((text: string) => void) | null): void;
  getSelectedText(): string | null;
  applyMarkup(kind: 'highlight' | 'underline' | 'strikeout' | 'squiggly'): string | null;
  hasTextSelection(): boolean;
  setOnTextSelectionChange(cb: (() => void) | null): void;
  setTool(kind: string): void;
  setHighlightColor(color: string): void;
  setMarkupColor(kind: string, color: string): void;
  deleteAnnotation(id: string): boolean;
  deleteSelected(): number;
}

// --- public SDK types ---

/** A font source: either a URL to fetch or inline bytes. */
export interface FontSource {
  url?: string;
  data?: Uint8Array;
}

/** The four text-markup annotation kinds (actions over a body-text selection). */
export type MarkupKind = 'highlight' | 'underline' | 'strikeout' | 'squiggly';

/** Viewport zoom mode (适应宽度/高度/页面 state machine). */
export type ZoomMode = 'custom' | 'fit-width' | 'fit-height' | 'fit-page';

/** Configuration for `Ofd.init`. */
export interface OfdConfig {
  /** Fonts to load + register whole (legacy channel). No default anymore —
   * bare init issues zero font requests (spec 2026-10-17 §3.1); hosts opt in
   * via this or `fontSlices`. */
  fonts?: FontSource[];
  /** 字体切片按需加载（spec 2026-10-17 §3.1）。设置后启用切片通道；
   * `fonts` 优先级更低（并存合法但建议二选一）。 */
  fontSlices?: FontSlicesConfig;
  /** Fired when the document changes (signal-only; on-demand rendering repaints). */
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
  /** Initial zoom mode. Fit modes apply once the viewport is laid out
   * (the first resize after init). */
  zoomMode?: Exclude<ZoomMode, 'custom'>;
  /** Fired when the zoom mode changes (entering a fit mode, or a manual
   * zoom exiting to 'custom'). Receives the new mode string. */
  onZoomModeChange?: (mode: ZoomMode) => void;
  /** Fired on Ctrl+C with body text selected (TextSelect tool). Defaults to
   * writing the text to the system clipboard via navigator.clipboard
   * (inside the user-activation window); pass `clipboard: false` and use
   * this to handle copying yourself. */
  onCopy?: (text: string) => void;
  /** Fired when the body-text selection appears/changes/clears (signal-only;
   * query hasTextSelection()/getSelectedText() afterwards). */
  onTextSelectionChange?: () => void;
  /** Set false to disable the default Ctrl+C -> clipboard wiring. */
  clipboard?: boolean;
}

/**
 * Wrap a host callback so it runs in a microtask instead of synchronously.
 *
 * Rust fires callbacks from inside wasm exports that hold a mutable borrow
 * on the WasmOfd (e.g. handlePointerMove -> text-selection change). A
 * handler that immediately calls back into the same editor (querying state,
 * saving, ...) re-enters that borrow and wasm-bindgen throws
 * "recursive use of an object detected". Deferring to a microtask runs the
 * handler after the export has returned and the borrow is released; the
 * "signal-only, query afterwards" callback contract relies on this.
 *
 * Microtasks still run within the same task tick, so browser user
 * activation (clipboard writes etc.) is preserved.
 */
function deferCb<A extends unknown[]>(cb: (...args: A) => void): (...args: A) => void {
  return (...args: A) => {
    queueMicrotask(() => cb(...args));
  };
}

/**
 * rofd web OFD surface. Created via [`Ofd.init`]; the SDK owns the canvas,
 * the wasm surface, DOM event binding, and the on-demand render scheduler.
 *
 * Usage:
 * ```ts
 * const ofd = await Ofd.init(container, {
 *   onSaveRequest: () => download(ofd.saveOfd()),
 * });
 * ofd.loadOfd(ofdBytes);
 * ```
 */
export class Ofd {
  private wasm: WasmOfd;
  private canvas: HTMLCanvasElement;
  private scheduler: RenderScheduler;
  private abortController: AbortController;
  // Click counting (mirrors the native adapter): pointerdown's
  // `detail` is 0 for pointer events per the Pointer Events spec, so the
  // count is tracked here -- same 500ms window + 4px slop, cycling 1->2->3.
  private clickCount = 0;
  private lastClickTime = 0;
  private lastClickX = -Infinity;
  private lastClickY = -Infinity;
  /** 字体流状态：off=未启用 init=进行中 ready=watcher 已挂（失败回 off，
   * 下次 loadOfd 重试——spec §5 渲染不阻塞）。 */
  private fontFlowState: 'off' | 'init' | 'ready' = 'off';
  private fontWatch?: fontFlow.FontWatch;
  private fontSlicesCfg?: FontSlicesConfig;
  private onWarningCb?: (warnings: string[]) => void;

  private constructor(wasm: WasmOfd, canvas: HTMLCanvasElement) {
    this.wasm = wasm;
    this.canvas = canvas;
    this.scheduler = new RenderScheduler(() => this.wasm.renderFrame());
    this.abortController = new AbortController();
  }

  /** init 的字体分派（可测导出）：`fonts` 走 legacy 逐个 await 全量加载
   * （行为与切片改造前逐字节一致，失败 warn 后继续）；无 `fonts` 则零请求。
   * 切片通道不在这里——由实例 startFontFlow 驱动（需 proxied wasm + kick）。 */
  static async dispatchFonts(
    wasmOfd: { registerFont(bytes: Uint8Array): boolean } | undefined,
    fonts: FontSource[] | undefined,
  ): Promise<void> {
    for (const font of fonts ?? []) {
      try {
        const bytes = await loadFont(font);
        wasmOfd?.registerFont(bytes);
      } catch (e) {
        console.warn('[rofd] font load failed; text may not render', e);
      }
    }
  }

  /**
   * Initialize a new OFD surface inside `container`: loads the wasm module,
   * checks WebGPU, creates a canvas, initializes the wasm surface (WebGPU +
   * warmup), loads fonts (legacy whole fonts awaited here; the slice flow
   * starts after wire), wires callbacks, binds DOM events, and starts
   * on-demand rendering. Returns a ready-to-use `Ofd`.
   */
  static async init(
    container: HTMLElement,
    config?: OfdConfig,
  ): Promise<Ofd> {
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

    // 4. Create WasmOfd (async: WebGPU init + warmup).
    const wasmOfd = await wasm.create_wasm_ofd(canvas);

    // 5. Fonts: legacy whole fonts awaited here (byte-compatible behavior);
    //    slice mode starts after wire (progressive, kicks the scheduler).
    await Ofd.dispatchFonts(wasmOfd, config?.fonts);

    // 6. Register callbacks (deferred: handlers run in a microtask so they
    // may freely call back into the surface - see deferCb).
    // onChange 双消费者：宿主回调 + 字体 watcher kick（批注文本变更补片）。
    let fontKick: () => void = () => {};
    const userOnChange = config?.onChange;
    if (userOnChange || config?.fontSlices) {
      wasmOfd.setOnChange(deferCb(() => {
        userOnChange?.();
        fontKick();
      }));
    }
    if (config?.onSelectionChange) wasmOfd.setOnSelectionChange(deferCb(config.onSelectionChange));
    if (config?.onCursorChange) wasmOfd.setOnCursorChange(deferCb(config.onCursorChange));
    if (config?.onSaveRequest) wasmOfd.setOnSaveRequest(deferCb(config.onSaveRequest));
    if (config?.onContextMenu) wasmOfd.setOnContextMenu(deferCb(config.onContextMenu));
    if (config?.onWarning) wasmOfd.setOnWarning(deferCb(config.onWarning));
    if (config?.onAnnotationFocus) wasmOfd.setOnAnnotationFocus(deferCb(config.onAnnotationFocus));
    if (config?.onAnnotationInteract) wasmOfd.setOnAnnotationInteract(deferCb(config.onAnnotationInteract));
    if (config?.onPageChange) wasmOfd.setOnPageChange(deferCb(config.onPageChange));
    if (config?.onZoomChange) wasmOfd.setOnZoomChange(deferCb(config.onZoomChange));
    // The wasm bridge passes zoom_mode_str() values - exactly the ZoomMode
    // literals - so the string-typed callback safely narrows to ZoomMode.
    if (config?.onZoomModeChange) wasmOfd.setOnZoomModeChange(deferCb(config.onZoomModeChange as (mode: string) => void));
    if (config?.onTextSelectionChange) wasmOfd.setOnTextSelectionChange(deferCb(config.onTextSelectionChange));

    // The wasm side reports CSS cursor names directly
    // ("default"/"grab"/"grabbing"/"text"), so no
    // mapping is needed on the TS side.
    wasmOfd.setOnPointerCursor((shape: string) => {
      canvas.style.cursor = shape;
    });

    // Copy: on Ctrl+C with a live TextSelect selection, forward the text.
    // The default writes to the system clipboard; the subscription happens
    // here (at create time) so the callback chain fires from the keydown
    // event within the browser's user-activation window (microtask deferral
    // keeps it in the same tick; see deferCb).
    const onCopy =
      config?.onCopy ??
      (config?.clipboard === false
        ? null
        : (text: string) => {
            void navigator.clipboard.writeText(text);
          });
    if (onCopy) wasmOfd.setOnCopy(deferCb(onCopy));

    // Initial zoom mode: recorded now, applied on the first resize below
    // (the component defers fit until the viewport is laid out).
    if (config?.zoomMode) {
      wasmOfd.setZoomMode(config.zoomMode);
    }

    // 7. Create wrapper (proxy + events + watchdog) via the shared wiring.
    const ofd = Ofd.wire(wasmOfd, canvas);

    // Slice font flow (if configured) starts now: the instance holds the
    // proxied wasm font surface, so every registered slice kicks the
    // scheduler (first slice renders as soon as it lands).
    if (config?.fontSlices) {
      ofd.startFontFlow(config.fontSlices, config.onWarning);
    }
    fontKick = () => ofd.fontWatchKick();

    // Prevent the browser's native context menu on the canvas (right-click).
    canvas.addEventListener('contextmenu', (e) => e.preventDefault());

    // 8. Initial resize (eager direct render) + first frame kick.
    ofd.resize();
    ofd.kick();

    // 9. Focus the canvas so keyboard input works immediately.
    canvas.focus();

    return ofd;
  }

  /** Shared wiring for `init` and `__testNew`: proxy-wrap the wasm surface,
   * construct, bind DOM events, connect the kick closure. The kick closure
   * is redirectable because the scheduler lives on the Ofd instance, which
   * does not exist until after the proxy wraps the surface. */
  private static wire(wasmOfd: WasmOfd, canvas: HTMLCanvasElement): Ofd {
    let kickLoop: () => void = () => {};
    const proxied = withKick(wasmOfd, () => kickLoop());
    const ofd = new Ofd(proxied, canvas);
    ofd.bindEvents();
    kickLoop = () => ofd.kick();
    return ofd;
  }

  /** @internal Test seam: same wiring as init minus the wasm boot
   * (fonts/callbacks/resize). Does NOT kick - tests drive scheduling
   * explicitly. */
  static __testNew(wasm: WasmOfd, canvas: HTMLCanvasElement): Ofd {
    return Ofd.wire(wasm, canvas);
  }

  /** Destroy the surface: stop the scheduler, abort listeners, cancel the
   * font watcher, remove canvas. */
  destroy(): void {
    this.fontWatch?.cancel();
    this.scheduler.dispose();
    this.abortController.abort();
    if (this.canvas.parentNode) {
      this.canvas.parentNode.removeChild(this.canvas);
    }
  }

  // ─── Font slice flow (spec 2026-10-17 §3.2) ───────────────────────────────

  /** @internal watcher kick 缝（init 的 fontKick 闭包指向这里）。 */
  fontWatchKick(): void {
    this.fontWatch?.kick();
  }

  /** @internal 启动切片字体流（init 的 fontSlices 分支；测试接线直接驱动）。
   * onWarning 与其它宿主回调一致经 deferCb 延迟——宿主抛错只落在微任务里，
   * 不会沿 onError→ensureFonts 反向传播楔死 retryFontFlow 的状态机。 */
  startFontFlow(cfg: FontSlicesConfig, onWarning?: (warnings: string[]) => void): void {
    this.fontSlicesCfg = cfg;
    this.onWarningCb = onWarning ? deferCb(onWarning) : undefined;
    void this.retryFontFlow();
  }

  private async retryFontFlow(): Promise<void> {
    const cfg = this.fontSlicesCfg;
    // 流只从 off 态进入：init 中重入会双重拉流，ready 中重入会双挂
    // watcher（泄漏一个 300ms debounce 定时器）。
    if (!cfg || this.fontFlowState !== 'off') {
      return;
    }
    this.fontFlowState = 'init';
    const live = await fontFlow.filterLiveManifests(cfg.manifests);
    if (live.length === 0) {
      this.fontFlowState = 'off';
      return;
    }
    const runtime = new FontRuntime({
      manifests: live,
      includeDefaultNoto: cfg.includeDefaultNoto ?? false,
    });
    const wasmFont: fontFlow.WasmFontSurface = {
      collectFontRequirements: () => this.wasm.collectFontRequirements(),
      registerFontSlice: (b, f, s) => this.wasm.registerFontSlice(b, f, s),
    };
    const onError = (err: fontFlow.FontFlowError): void => {
      this.onWarningCb?.([`fonts: ${err.family ? `${err.family} ` : ''}${err.message}`]);
    };
    const ok = await fontFlow.ensureFonts(wasmFont, runtime, {
      defaultFamily: cfg.defaultFamily,
      onError,
    });
    if (!ok) {
      this.fontFlowState = 'off';
      return;
    }
    this.fontWatch = fontFlow.scheduleFontWatch(wasmFont, runtime, { onError });
    this.fontFlowState = 'ready';
  }

  // ─── Internal: event binding ──────────────────────────────────────────────

  /**
   * Compute the click count for a press at `(x, y)` (device px): presses
   * within 500ms and 4px of the previous one advance the chain 1->2->3->1
   * (double = word select, triple = paragraph select); anything else starts
   * over at 1. `e.detail` can't be used -- it is 0 for pointer events.
   */
  private nextClickCount(x: number, y: number): number {
    const now = performance.now();
    const withinWindow =
      now - this.lastClickTime <= 500 &&
      Math.abs(x - this.lastClickX) <= 4 &&
      Math.abs(y - this.lastClickY) <= 4;
    this.clickCount = withinWindow ? (this.clickCount % 3) + 1 : 1;
    this.lastClickTime = now;
    this.lastClickX = x;
    this.lastClickY = y;
    return this.clickCount;
  }

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
        // scroll by page_h + page_gap). Mirrors the native adapter.
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
        const x = (e.clientX - rect.left) * dpr();
        const y = (e.clientY - rect.top) * dpr();
        // Right-click resets the click chain (mirrors the native adapter).
        if (e.button !== 0) {
          this.clickCount = 0;
        }
        this.wasm.handleMouseDown(
          e.button,
          x,
          y,
          e.shiftKey,
          e.ctrlKey,
          e.altKey,
          e.metaKey,
          this.nextClickCount(x, y),
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
          // Pure horizontal scroll (deltaY === 0) with Ctrl is not a zoom gesture.
          if (Math.abs(e.deltaY) >= 1e-9) {
            this.wasm.handleZoomAt(e.deltaY > 0 ? 0.9 : 1.1, cx, cy);
          }
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

    // Watchdog (spec 2026-10-13 §7.4): hidden tabs are fully silenced -
    // Chromium still releases ~1 rAF/s for hidden pages with pending
    // frames and aligns timers to 1s slots (rword pitfall 2), so gating
    // alone is not enough. Returning to visible or window focus kicks.
    document.addEventListener(
      'visibilitychange',
      () => {
        if (document.hidden) {
          this.scheduler.suspendForHidden();
        } else {
          this.scheduler.resumeFromHidden();
        }
      },
      opts,
    );
    window.addEventListener('focus', () => this.scheduler.kick(), opts);
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

  // ─── Public API ───────────────────────────────────────────────────────────

  /** @internal Ensure a frame is scheduled (idempotent). The kick proxy
   * calls this after every wasm method; the watchdog calls it on page
   * visibility/focus changes. */
  kick(): void {
    this.scheduler.kick();
  }

  /** Stop rendering and drop all pending work (host policy for hiding the
   * component: hidden panels, inactive tabs). Paired with `resumeRendering`. */
  pauseRendering(): void {
    this.scheduler.pause();
  }

  /** Resume after `pauseRendering`; schedules one frame immediately. No-op
   * when already running or destroyed. */
  resumeRendering(): void {
    this.scheduler.resume();
  }

  /** Load an OFD document from raw `.ofd` package bytes. */
  loadOfd(bytes: Uint8Array): void {
    this.wasm.loadOfd(bytes);
    // 文档换了需求全变：立即 kick watcher；流此前失败则重试（spec §3.2）。
    if (this.fontFlowState === 'ready') {
      this.fontWatch?.kick();
    } else if (this.fontFlowState === 'off' && this.fontSlicesCfg) {
      void this.retryFontFlow();
    }
  }

  /** Serialize the current document to OFD package bytes. */
  saveOfd(): Uint8Array {
    return this.wasm.saveOfd();
  }

  /** Set the annotation clock (author + timestamp ms) for subsequent edits. */
  setClock(author: string, ts: number): void {
    // i64 maps to BigInt in wasm-bindgen; convert from JS number.
    this.wasm.setClock(author, BigInt(ts));
  }

  /** Set the active editing tool. `kind` is one of: "text", "hand",
   * "freehand", "rect", "ellipse", "arrow", "line", "polygon".
   * "select"/"textSelect" are accepted as aliases of "text" (the
   * unified tool: selects annotations AND drag-selects body text).
   * Markup values ("highlight", "underline", "strikeout", "squiggly")
   * are no longer tools and fall back to "text", like unknown values -
   * use applyMarkup(kind) on the current body-text selection instead. */
  setTool(kind: string): void {
    this.wasm.setTool(kind);
  }

  /** Set the color used when applyMarkup('highlight') creates a new
   * annotation. `color` is "#RRGGBB" (invalid strings fall back to black).
   * Mirrors the highlight-color dropdown on the annotate tab. */
  setHighlightColor(color: string): void {
    this.wasm.setHighlightColor(color);
  }

  /** Set the color applyMarkup(kind) uses for new annotations (each markup
   * kind gets its own color dropdown). `kind` is one of "highlight",
   * "underline", "strikeout", "squiggly"; other kinds are ignored.
   * `color` is "#RRGGBB" (invalid strings fall back to black). */
  setMarkupColor(kind: string, color: string): void {
    this.wasm.setMarkupColor(kind, color);
  }

  /** The current body-text selection's text (Text tool), or null when
   * there is no selection. */
  getSelectedText(): string | null {
    return this.wasm.getSelectedText();
  }

  /** Convert the current body-text selection into a markup annotation of the
   * given kind. Color is the per-kind default (setMarkupColor). Returns the
   * new annotation's id, or null when there is no selection. The selection is
   * kept so further markups can stack on the same range. */
  applyMarkup(kind: MarkupKind): string | null {
    return this.wasm.applyMarkup(kind);
  }

  /** Whether a body-text selection currently exists (markup buttons' enabled
   * state). */
  hasTextSelection(): boolean {
    return this.wasm.hasTextSelection();
  }

  /** Fired when the body-text selection appears/changes/clears. Signal-only;
   * query hasTextSelection()/getSelectedText() afterwards. Handlers are
   * deferred to a microtask (see deferCb) so they may call back into the
   * surface without re-entering the wasm borrow. */
  setOnTextSelectionChange(cb: (() => void) | null): void {
    this.wasm.setOnTextSelectionChange(cb && deferCb(cb));
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

  /** Undo the last command (toolbar button; same as Ctrl+Z). Returns
   * whether anything was undone. */
  undo(): boolean {
    return this.wasm.undo();
  }

  /** Redo the last undone command (toolbar button; same as Ctrl+Y).
   * Returns whether anything was redone. */
  redo(): boolean {
    return this.wasm.redo();
  }

  /** Whether there are undoable operations in the history. */
  canUndo(): boolean {
    return this.wasm.canUndo();
  }

  /** Whether there are redoable operations in the history. */
  canRedo(): boolean {
    return this.wasm.canRedo();
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

  /** Set the viewport zoom mode: 'fit-width' | 'fit-height' | 'fit-page'
   * enters a fit mode (re-derived on resize / document changes), 'custom'
   * exits. Manual zoom (Ctrl+wheel, percent presets) also exits to
   * 'custom'. */
  setZoomMode(mode: ZoomMode): void {
    this.wasm.setZoomMode(mode);
  }

  /** The current zoom mode string. */
  zoomMode(): ZoomMode {
    return this.wasm.zoomMode() as ZoomMode;
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
