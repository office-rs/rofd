# C Editor*→Ofd* 家族更名 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 把 Editor* 界面/组件家族全部更名为 Ofd*：Rust 核心 `EditorComponent→OfdComponent`、`EditorConfig→OfdConfig`；web-view `WasmEditor→WasmOfd`、`create_wasm_editor→create_wasm_ofd`；SDK TS `class Editor→class Ofd`（版本 0.1.6）；Vue 宿主变量/导入同步；文档终态更新。不留别名。

**Architecture:** 6 个任务对应 spec §5.4 的 6 个提交，每个独立绿色：C1 核心 + 全部非 web-view 调用点（wasm 检查有意推迟）；C2 web-view Rust（恢复 wasm 门禁）；C3 SDK TS/package.json/README；C4 Vue；C5 文档；C6 终验 + 全仓旧名零命中审计。

**Tech Stack:** sed（严格按 spec §5.3 纪律）、wasm-pack/tsc、vite。参照 rword commit `26db757`（Rust）、`4017f98`（SDK）、`34235a0`（Vue）、`b456ed4`（文档）。

**Spec:** [`docs/superpowers/specs/2026-09-22-rofd-xilem-refactor-and-rename-design.md`](../specs/2026-09-22-rofd-xilem-refactor-and-rename-design.md) §5.2（映射）、§5.3（sed 纪律）、§5.4（提交切分）、§6（门禁）、§7（文档）。

## Global Constraints

- 前置条件：transform B 已提交（`rofd-xilem-view`/`xilem-app` 目录与包名就位）且门禁绿色。
- 单一 main 分支，直接提交 main；conventional commits；提交信息不加 attribution 行。
- **保留不改名（spec §5.2）**：`rofd-editor` crate 名与核心 `Editor` 结构体（`crates/editor/src/editor.rs`）；`command_queue()`；prose 普通名词 "editor"；历史文档（`docs/superpowers/`、`tmp/`）内容不改。
- **绝不**对 Rust 侧裸 `\bEditor\b` 全局替换；本计划所有 sed 模式均为复合全名，不使用裸 Editor 模式。
- 每步 sed 后立即跑该步旧名 grep 审计，不信任清单（spec §5.3.6）。
- 行为零变化：不碰业务逻辑、不碰测试断言（只改名称/路径）。
- **前端门禁基础设施现状（spec §6.5 的本仓库适配）**：SDK 无测试框架（devDeps 仅 @webgpu/types + typescript）——C3 门禁用 `npm run build:ts`（tsc 即类型检查），无 `npm test`；web-app 无 type-check 脚本、无 vue-tsc——C4 补 `"type-check": "vue-tsc --noEmit"` 脚本与 vue-tsc devDep（对齐 rword web-app），门禁跑 `npm run type-check && npm run build`。

## File Structure

| 文件 | 动作 | 任务 |
| --- | --- | --- |
| `crates/component/src/editor_component.rs` | git mv → `ofd_component.rs` + sed | C1 |
| `crates/component/src/lib.rs` | sed（含 mod 路径） | C1 |
| `crates/component/src/config.rs` | sed | C1 |
| `crates/component/tests/integration.rs` | sed | C1 |
| `crates/component/tests/sample_drag_select.rs` | sed | C1 |
| `crates/xilem-view/src/ofd_widget.rs` | sed | C1 |
| `crates/xilem-view/src/ofd_view.rs` | sed | C1 |
| `crates/xilem-view/tests/ofd_widget_harness.rs` | sed | C1 |
| `crates/xilem-app/src/main.rs` | sed | C1 |
| `crates/web-view/src/wasm_editor.rs` | git mv → `wasm_ofd.rs` + sed | C2 |
| `crates/web-view/src/lib.rs` | sed | C2 |
| `crates/web-view/sdk/src/index.ts` | 全文替换 | C3 |
| `crates/web-view/sdk/package.json` | version → 0.1.6 | C3 |
| `crates/web-view/sdk/README.md` | sed + 定点编辑 | C3 |
| `crates/web-app/src/App.vue` | 定点编辑 | C4 |
| `crates/web-app/src/main.ts` | 1 处注释 | C4 |
| `AGENTS.md` | 全文替换 | C5 |
| `README.md` / `README.zh-CN.md` | 定点编辑 | C5 |
| `CHANGELOG.md` | 顶部新增条目 | C5 |

---

### Task 1 (C1): Rust 核心 OfdComponent/OfdConfig + 全部非 web-view 调用点

**Files:** 见 File Structure C1 行。

**Interfaces:**
- Produces: `rofd_component::OfdComponent`、`rofd_component::OfdConfig`；文件 `crates/component/src/ofd_component.rs`。
- Staged note: 本任务后 `rofd-web-view` 的 wasm32-gated 代码仍引用旧名，native 构建不受影响（全部在 `#[cfg(target_arch = "wasm32")] mod wasm_impl` 内）；wasm check 在 C2 恢复。

- [ ] **Step 1: 文件改名**

```bash
git mv crates/component/src/editor_component.rs crates/component/src/ofd_component.rs
```

- [ ] **Step 2: sed 两个复合全名（全部非 web-view 文件）**

```bash
FILES="crates/component/src/config.rs crates/component/src/ofd_component.rs \
crates/component/tests/integration.rs crates/component/tests/sample_drag_select.rs \
crates/xilem-view/src/ofd_widget.rs crates/xilem-view/src/ofd_view.rs \
crates/xilem-view/tests/ofd_widget_harness.rs crates/xilem-app/src/main.rs"
sed -i 's/\bEditorComponent\b/OfdComponent/g; s/\bEditorConfig\b/OfdConfig/g' $FILES
```

- [ ] **Step 3: lib.rs 改名（含 mod 路径）**

```bash
sed -i 's/\bEditorComponent\b/OfdComponent/g; s/\bEditorConfig\b/OfdConfig/g; s/\beditor_component\b/ofd_component/g' \
  crates/component/src/lib.rs
```

- [ ] **Step 4: 旧名审计（crates，排除 web-view）**

Run:

```bash
! rg -n "\bEditorComponent\b|\bEditorConfig\b|editor_component" \
  -g '!crates/web-view/**' crates
```

Expected: 无匹配。若有命中（如 CJK 全角标点旁手工补），处理后重跑。

- [ ] **Step 5: native 门禁（本步不做 wasm check）**

```bash
cargo build --workspace
cargo test --workspace --exclude tauri-app --exclude rofd-web-view
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
```

Expected: 全绿。`cargo build --workspace` 编译 rofd-web-view 的 native 面（非 gated 代码），通过；其 wasm-only 旧名代码不参与 native 编译。

- [ ] **Step 6: 提交**

```bash
git add -A
git commit -m "refactor(component): EditorComponent/EditorConfig → OfdComponent/OfdConfig"
```

---

### Task 2 (C2): web-view Rust——WasmOfd/create_wasm_ofd + 文件改名

**Files:**
- Move/Modify: `crates/web-view/src/wasm_editor.rs` → `wasm_ofd.rs`
- Modify: `crates/web-view/src/lib.rs`

**Interfaces:**
- Produces: wasm 导出类 `WasmOfd`、工厂 `create_wasm_ofd`；模块路径 `crate::wasm_ofd`。

- [ ] **Step 1: 文件改名**

```bash
git mv crates/web-view/src/wasm_editor.rs crates/web-view/src/wasm_ofd.rs
```

- [ ] **Step 2: sed——长名先于短名**

对移动后的文件：

```bash
sed -i 's/\bWasmEditor\b/WasmOfd/g; s/\bcreate_wasm_editor\b/create_wasm_ofd/g; s/\bwasm_editor\b/wasm_ofd/g; s/\bEditorComponent\b/OfdComponent/g; s/\bEditorConfig\b/OfdConfig/g' \
  crates/web-view/src/wasm_ofd.rs
```

对 lib.rs：

```bash
sed -i 's/\bWasmEditor\b/WasmOfd/g; s/\bcreate_wasm_editor\b/create_wasm_ofd/g; s/\bwasm_editor\b/wasm_ofd/g; s/\bEditorComponent\b/OfdComponent/g; s/\bEditorConfig\b/OfdConfig/g' \
  crates/web-view/src/lib.rs
```

说明：`\bwasm_editor\b` 只命中模块路径/文件名引用，不碰 prose（prose 是 "wasm editor" 带空格）。

- [ ] **Step 3: 旧名审计（web-view src）**

Run:

```bash
! rg -n "\bWasmEditor\b|\bcreate_wasm_editor\b|\bwasm_editor\b|\bEditorComponent\b|\bEditorConfig\b" \
  crates/web-view/src
```

Expected: 无匹配。逐行看一眼 lib.rs 的模块声明/再导出/工厂签名，确认已变为 `pub mod wasm_ofd;`、`pub use wasm_ofd::WasmOfd;`、`pub async fn create_wasm_ofd(...) -> Result<wasm_ofd::WasmOfd, JsValue>`。

- [ ] **Step 4: 门禁（恢复 wasm check）**

```bash
cargo build --workspace
cargo test --workspace --exclude tauri-app --exclude rofd-web-view
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
cargo check -p rofd-web-view --target wasm32-unknown-unknown
```

Expected: 全绿，wasm32 下 WasmOfd/OfdComponent 编译通过。

- [ ] **Step 5: 提交**

```bash
git add -A
git commit -m "refactor(web-view): WasmEditor/create_wasm_editor → WasmOfd/create_wasm_ofd"
```

---

### Task 3 (C3): SDK TS——Ofd 类 + package.json 0.1.6 + README

**Files:**
- Modify: `crates/web-view/sdk/package.json`
- Modify: `crates/web-view/sdk/src/index.ts`（全文替换）
- Modify: `crates/web-view/sdk/README.md`

**Interfaces:**
- Produces: `export class Ofd`、`Ofd.init(container, config?: OfdConfig): Promise<Ofd>`、`export interface OfdConfig`。

- [ ] **Step 1: package.json version 0.1.6**

把 `crates/web-view/sdk/package.json` 中：

```json
  "version": "0.2.0",
  "description": "TypeScript SDK wrapping the rofd WASM editor exports.",
```

替换为：

```json
  "version": "0.1.6",
  "description": "TypeScript SDK wrapping the rofd WASM OFD exports.",
```

- [ ] **Step 2: index.ts 全文替换**

把 `crates/web-view/sdk/src/index.ts` 全文替换为：

```ts
// @office-rs/rofd - TypeScript SDK wrapping the rofd WASM OFD surface.
//
// Mirrors the as-built adapter design: `Ofd.init(container, config)` does the
// full boot (load wasm -> check WebGPU -> create canvas -> create_wasm_ofd ->
// load + register fonts -> register callbacks -> bindEvents -> render loop).
// The web app just calls `init` and optionally passes fonts/callbacks.

// --- wasm module shape (wasm-pack --target web) ---
// `default` is the async init; `create_wasm_ofd` is the factory.
type WasmModule = {
  default(): Promise<void>;
  create_wasm_ofd(canvas: HTMLCanvasElement): Promise<WasmOfd>;
};

interface WasmOfd {
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

/** Configuration for `Ofd.init`. */
export interface OfdConfig {
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

// Default font CDN (jsDelivr - ICP-licensed China CDN nodes). Same fonts the
// sibling adapters use. The web can't access system fonts, so these are the
// only font source.
const FONT_CDN_BASE =
  'https://cdn.jsdelivr.net/gh/googlefonts/noto-cjk@main/Sans/OTF/SimplifiedChinese';
const DEFAULT_FONTS: FontSource[] = [
  { url: `${FONT_CDN_BASE}/NotoSans-Regular.ttf` },
  { url: `${FONT_CDN_BASE}/NotoSansCJKsc-Regular.otf` },
];

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
 * the wasm surface, DOM event binding, and the render loop.
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
  private animFrameId: number | null = null;
  private abortController: AbortController;
  // Click counting (mirrors the native adapter): pointerdown's
  // `detail` is 0 for pointer events per the Pointer Events spec, so the
  // count is tracked here -- same 500ms window + 4px slop, cycling 1->2->3.
  private clickCount = 0;
  private lastClickTime = 0;
  private lastClickX = -Infinity;
  private lastClickY = -Infinity;

  private constructor(wasm: WasmOfd, canvas: HTMLCanvasElement) {
    this.wasm = wasm;
    this.canvas = canvas;
    this.abortController = new AbortController();
  }

  /**
   * Initialize a new OFD surface inside `container`: loads the wasm module,
   * checks WebGPU, creates a canvas, initializes the wasm surface (WebGPU +
   * warmup), loads + registers fonts, wires callbacks, binds DOM events,
   * and starts the render loop. Returns a ready-to-use `Ofd`.
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

    // 5. Load + register fonts (web can't access system fonts).
    const fonts = config?.fonts ?? DEFAULT_FONTS;
    for (const font of fonts) {
      try {
        const bytes = await loadFont(font);
        wasmOfd.registerFont(bytes);
      } catch (e) {
        console.warn('[rofd] font load failed; text may not render', e);
      }
    }

    // 6. Register callbacks (deferred: handlers run in a microtask so they
    // may freely call back into the surface - see deferCb).
    if (config?.onChange) wasmOfd.setOnChange(deferCb(config.onChange));
    if (config?.onSelectionChange) wasmOfd.setOnSelectionChange(deferCb(config.onSelectionChange));
    if (config?.onCursorChange) wasmOfd.setOnCursorChange(deferCb(config.onCursorChange));
    if (config?.onSaveRequest) wasmOfd.setOnSaveRequest(deferCb(config.onSaveRequest));
    if (config?.onContextMenu) wasmOfd.setOnContextMenu(deferCb(config.onContextMenu));
    if (config?.onWarning) wasmOfd.setOnWarning(deferCb(config.onWarning));
    if (config?.onAnnotationFocus) wasmOfd.setOnAnnotationFocus(deferCb(config.onAnnotationFocus));
    if (config?.onAnnotationInteract) wasmOfd.setOnAnnotationInteract(deferCb(config.onAnnotationInteract));
    if (config?.onPageChange) wasmOfd.setOnPageChange(deferCb(config.onPageChange));
    if (config?.onZoomChange) wasmOfd.setOnZoomChange(deferCb(config.onZoomChange));
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

    // 7. Create wrapper + bind DOM events.
    const ofd = new Ofd(wasmOfd, canvas);
    ofd.bindEvents();

    // Prevent the browser's native context menu on the canvas (right-click).
    canvas.addEventListener('contextmenu', (e) => e.preventDefault());

    // 8. Initial resize + render loop.
    ofd.resize();
    ofd.startRenderLoop();

    // 9. Focus the canvas so keyboard input works immediately.
    canvas.focus();

    return ofd;
  }

  /** Destroy the surface: stop the render loop, remove canvas, abort listeners. */
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
```

- [ ] **Step 3: SDK README 改名**

先跑安全的 sed（全部为复合模式）：

```bash
sed -i 's/`Editor\.init`/`Ofd.init`/g; s/\bEditorConfig\b/OfdConfig/g' \
  crates/web-view/sdk/README.md
sed -i 's/^## Editor API/## Ofd API/' crates/web-view/sdk/README.md
sed -i 's/Callbacks (passed to `Editor\.init`)/Callbacks (passed to `Ofd.init`)/' \
  crates/web-view/sdk/README.md
sed -i 's/^  import { Editor }/  import { Ofd }/' crates/web-view/sdk/README.md
```

再定点处理 Quick start 代码块。把：

```html
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
```

替换为：

```html
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
```

以及末尾自定义字体示例中把：

```ts
await Editor.init(container, {
```

替换为：

```ts
await Ofd.init(container, {
```

- [ ] **Step 4: 审计与门禁**

```bash
! rg -n "\bclass Editor\b|\bEditor\.init\b|\bEditorConfig\b|\bWasmEditor\b" \
  crates/web-view/sdk/src crates/web-view/sdk/README.md
cd crates/web-view/sdk && npm run build:ts
```

Expected: 零命中；tsc 编译通过并产出 dist 声明。若本机有 wasm-pack 且 dist 需更新，另跑 `npm run build`（wasm 产物名不变：`rofd_web_view`）。

- [ ] **Step 5: 提交**

```bash
git add -A
git commit -m "refactor(sdk): JS Editor 类更名 Ofd，版本 0.1.6，README 同步"
```

---

### Task 4 (C4): Vue 宿主——App.vue + main.ts

**Files:**
- Modify: `crates/web-app/src/App.vue`
- Modify: `crates/web-app/src/main.ts`
- Modify: `crates/web-app/package.json`

- [ ] **Step 1: import 改名**

把 `crates/web-app/src/App.vue` 中：

```ts
import { Editor } from '@office-rs/rofd';
```

替换为：

```ts
import { Ofd } from '@office-rs/rofd';
```

- [ ] **Step 2: save() 块先改（在 replace_all 之前）**

把：

```ts
async function save(): Promise<void> {
  const ed = editor.value;
  if (!ed) return;
  const ok = await fileHost.save(ed.saveOfd(), 'document.ofd');
  if (ok) message.success('已保存 document.ofd');
}
```

替换为：

```ts
async function save(): Promise<void> {
  const ofdInst = ofd.value;
  if (!ofdInst) return;
  const ok = await fileHost.save(ofdInst.saveOfd(), 'document.ofd');
  if (ok) message.success('已保存 document.ofd');
}
```

- [ ] **Step 3: onMounted init 块**

把：

```ts
    const ed = await Editor.init(containerRef.value!, {
```

替换为：

```ts
    const ofdInst = await Ofd.init(containerRef.value!, {
```

把：

```ts
    editor.value = ed;
    ed.setClock('rofd', Date.now());
```

替换为：

```ts
    ofd.value = ofdInst;
    ofdInst.setClock('rofd', Date.now());
```

把：

```ts
      if (res.ok) ed.loadOfd(new Uint8Array(await res.arrayBuffer()));
```

替换为：

```ts
      if (res.ok) ofdInst.loadOfd(new Uint8Array(await res.arrayBuffer()));
```

- [ ] **Step 4: 声明行改名**

把：

```ts
const editor = shallowRef<Editor | null>(null);
```

替换为：

```ts
const ofd = shallowRef<Ofd | null>(null);
```

- [ ] **Step 5: 其余 editor.value 一次性改名**

把本文件中剩余的全部 `editor.value` 替换为 `ofd.value`（replace_all）。覆盖 setTool/applyMarkup/undo/redo/zoom/scroll/delete/color/load/destroy 等调用点及生命周期收尾。

说明：`console.error('[rofd] editor init failed:', e)` 中的 editor 是普通名词行文，保持不动。

- [ ] **Step 6: 源文件路径注释**

把：

```ts
// （见 component/src/editor_component.rs 的 `zoom: PX_PER_MM` 初始化）。
```

替换为：

```ts
// （见 component/src/ofd_component.rs 的 `zoom: PX_PER_MM` 初始化）。
```

- [ ] **Step 7: main.ts 注释**

把 `crates/web-app/src/main.ts` 中：

```ts
// SDK（Editor.init）持有 canvas、WebGPU 初始化、字体加载、DOM 事件绑定与
```

替换为：

```ts
// SDK（Ofd.init）持有 canvas、WebGPU 初始化、字体加载、DOM 事件绑定与
```

- [ ] **Step 8: package.json 加 type-check 门禁（对齐 rword web-app）**

把 `crates/web-app/package.json` 中：

```json
  "scripts": {
    "dev": "vite",
    "build": "vite build",
    "build:sdk": "cd ../web-view && wasm-pack build --target web --out-dir sdk/dist"
  },
```

替换为：

```json
  "scripts": {
    "dev": "vite",
    "build": "vite build",
    "build:sdk": "cd ../web-view && wasm-pack build --target web --out-dir sdk/dist",
    "type-check": "vue-tsc --noEmit"
  },
```

把 devDependencies：

```json
  "devDependencies": {
    "@vitejs/plugin-vue": "^5.2.4",
    "typescript": "^5.0.0",
    "vite": "^5.0.0"
  }
```

替换为：

```json
  "devDependencies": {
    "@vitejs/plugin-vue": "^5.2.4",
    "typescript": "^5.0.0",
    "vite": "^5.0.0",
    "vue-tsc": "^2.1.10"
  }
```

然后 `cd crates/web-app && npm install`（安装 vue-tsc）。

- [ ] **Step 9: 审计与门禁**

```bash
! rg -n "\bEditor\b|editor\.value|\bed\b" crates/web-app/src/App.vue crates/web-app/src/main.ts
cd crates/web-app && npm run type-check && npm run build
```

Expected: 零命中（若命中全是注释里的普通名词行文，逐行核对后保留）；vue-tsc 零类型错误、vite 构建通过。tauri-app 前端复用本文件，无独立改名面。

- [ ] **Step 10: 提交**

```bash
git add -A
git commit -m "refactor(web-app): Vue 宿主 Editor→Ofd（导入、变量、调用点、type-check）"
```

---

### Task 5 (C5): 文档终态——AGENTS/README/CHANGELOG

**Files:**
- Modify: `AGENTS.md`（全文替换）
- Modify: `README.md`
- Modify: `README.zh-CN.md`
- Modify: `CHANGELOG.md`

- [ ] **Step 1: AGENTS.md 全文替换**

把 `AGENTS.md` 全文替换为：

```markdown
# AGENTS.md

本文件为在本仓库工作的 AI 编程代理（以及人类协作者）提供上下文。先读这里，再动代码。

设计 spec 是事实的最终来源：最新一份 [`docs/superpowers/specs/2026-09-22-rofd-xilem-refactor-and-rename-design.md`](docs/superpowers/specs/2026-09-22-rofd-xilem-refactor-and-rename-design.md)。本文档与代码现状对齐；二者冲突时以代码 + spec 为准，并回来修本文档。

---

## 1. 项目是什么

**rofd** 是一个 OFD（GB/T 33190）**查看 + 批注**编辑器**库**，Rust 实现，双平台（native + WASM）。as-built 参照项目是 `D:/code/rword`（OOXML 编辑器库，同型改造已全部落地）。

- **v1 范围**：查看 + 批注。主文档（body）**只读渲染**；批注是**唯一可变层**。
- **库形态**：以 `OfdComponent` 为唯一集成入口（类比 `<textarea>`），宿主控制消息循环并转发事件。库本身不取系统时间、不直接依赖 GUI 框架。
- **平台边界**：库 = 平台无关核心（dom/io/render/editor/component）+ 两个平台适配器（xilem-view/web-view）；`crates/xilem-app`、`crates/web-app` 与 `crates/tauri-app` 是宿主应用，不是库的交付物（见 §4.9）。
- **非目标**：编辑 body 内容、创建/验签电子签名、全保真渲染（模板继承/JBIG2/瓦片图按需补，v1 桩处理）、实时协同。
- 为 B（后端生成）/ C（PDF→OFD）/ D（后端读改写）留门，但不实现。

---

## 2. 常用命令

> 工作目录始终为仓库根 `D:\code\rofd`，除非另说明。Shell 为 bash（Unix 语法）。

### 构建 / 测试

```bash
cargo build --workspace          # 全量编译
cargo test  --workspace --exclude tauri-app --exclude rofd-web-view
cargo test  -p rofd-io           # 单 crate 测试
cargo test  -p rofd-io surgical  # 按名过滤（手术刀字节保留测试）
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
```

覆盖率目标 80%：`cargo llvm-cov --workspace`（需安装 `cargo-llvm-cov`）。

### 运行 native 宿主

```bash
cargo run -p xilem-app                           # 空编辑器
cargo run -p xilem-app -- test/ru-yuan-ji-lu.ofd  # 打开根目录 test/ 下的 .ofd
```

命令行路径参数经宿主 `host::document_io::load_ofd` 加载。xilem-app 按相对 CWD 的候选路径查找默认 CJK 字体（`crates/web-app/public/NotoSansSC-Regular.otf`）。若未下载，文字不渲染但程序不崩——先跑下面 web-app 的 `npm run fetch:font` 即可获得该字体文件。

### 构建 / 运行 web 宿主

```bash
rustup target add wasm32-unknown-unknown          # 一次性
cd crates/web-app
npm install
npm run fetch:font        # 下载 NotoSansSC 到 public/（文字渲染必需）
npm run build:sdk         # wasm-pack build crates/web-view -> sdk/dist
npm run dev               # vite 开发服务器
npm run build             # vite 生产构建
```

`build:sdk` 等价于 `cd ../web-view && wasm-pack build --target web --out-dir sdk/dist`。

### 构建 / 运行 tauri 桌面宿主

```bash
cd crates/tauri-app
npm install
npm run build:sdk         # 同 web-app：wasm-pack build crates/web-view -> sdk/dist
npm run tauri dev         # 开发：起 vite(1420) + 系统 WebView 窗口
npm run tauri build       # 打包：vite build -> tauri 打成桌面安装包
```

tauri-app 前端**复用 web-app 源码**（`main.ts` 从 `../web-app/src` 导入 App.vue）；仅在挂载前用 `setFileHost` 注入原生文件桥（`src/host/tauri.ts`，走 tauri-plugin-dialog + tauri-plugin-fs 的原生打开/保存对话框）。字体与 sample.ofd 从 web-app 的 `public/` 本地加载（vite `publicDir` 指向它），脱离 CDN。`src-tauri` 的 Rust 壳只启动窗口 + 注册插件，**不依赖任何 rofd crate**（§4.9）。平台支持：Windows（WebView2）开箱即用；macOS/Linux 受 WebKit 的 WebGPU 进度限制。

产物命名统一为 `rofd`（详见 release-tauri.yml）。

---

## 3. 仓库布局与分层

严格 5 层单向依赖，**反向边禁止**。每个 crate 的 `[dependencies]` 是依赖方向的唯一事实来源。**平台边界线画在 component 与适配器之间**：component 及以下五个 crate 平台无关（不依赖 winit/web-sys/wgpu/arboard 等平台 crate），xilem-view / web-view 是仅有的两层平台绑定。

```
crates/xilem-app      ─┐
                       ├─► xilem-view ─┐
                       │                ├─► component ─┬─► render ──┐
crates/web-app ─► web-view ─────────────┘              ├─► editor ──┴─► dom
   (JS + Vite,         (wasm-bindgen     │              │
    非 cargo 成员)      适配器)           │              └─► io ────────┘
```

| crate            | 路径                 | 职责 | 依赖（rofd 内部） |
| ---------------- | -------------------- | --- | ----------------- |
| `rofd-dom`       | `crates/dom`         | 纯数据模型 | — |
| `rofd-io`        | `crates/io`          | `parse_ofd` / `save_ofd`（手术刀）/ `write_ofd`（全量）+ `PackageHandle` | dom |
| `rofd-render`    | `crates/render`      | `imaging::record::Scene` 构建 + hit_test/caret_rect | dom |
| `rofd-editor`    | `crates/editor`      | 批注选区、命令模式、Step/Transaction/History | dom |
| `rofd-component` | `crates/component`   | **唯一集成入口** `OfdComponent`：ViewEvent、Callbacks、脏缓存 | dom + render + editor（**不依赖 io**） |
| `rofd-xilem-view`| `crates/xilem-view`  | masonry/xilem 薄适配器：`OfdWidget` + `ofd()`/`ofd_with_config()` | component |
| `rofd-web-view`  | `crates/web-view`    | WASM 薄适配器：`WasmOfd` + `WebGpuRenderTarget` + TS SDK | component + io + dom + vello + imaging + imaging_vello + wgpu + web-sys |
| `xilem-app`      | `crates/xilem-app`   | xilem 宿主应用（文件对话框 + 工具栏 UI 策略） | xilem-view + component + io + dom + xilem + rfd |
| web-app          | `crates/web-app`     | Vite + TS 宿主应用（非 cargo 成员） | `@office-rs/rofd`（= `crates/web-view/sdk`） |
| `tauri-app`      | `crates/tauri-app`   | Tauri 桌面宿主应用 | tauri 壳不依赖 rofd crate；前端复用 web-app |

> **两条历史偏离（改造后的 as-built）**：
> 1. component 保持 io-free——io 依赖从不下沉到组件；
> 2. native 侧 io 调用已**上移到宿主**（`xilem-app/src/host/document_io.rs`），适配器 rofd-xilem-view 不依赖 io；web-view 侧仍持 io（wasm 宿主自己就是"宿主"）。

---

## 4. 不可违反的不变量

### 4.1 依赖严格向上，反向边禁止

### 4.2 body 只读；批注是唯一可变面

### 4.3 手术刀保存：未触碰条目字节级保留

`save_ofd(doc, pkg)`：批注条目从 `AnnotationModel` 重新序列化；`Document.xml` 字节级打补丁（`<MaxUnitID>` + 缺失时插入 `<Annotations>` loc，严格阅读器只经此引用发现批注）；其余条目原样拷字节。改 io 保存逻辑后，核心测试（`crates/io/tests/round_trip.rs`、`save_surgical.rs`）必须仍绿。宿主层另有 `crates/xilem-app/tests/c2_save.rs`（#[ignore]，真实样例）。

### 4.4 库不取系统时间

库内**绝不**调 `Date::now()` / `SystemTime::now()`。宿主通过 `set_clock(author, ts)` 注入；命令用注入时间填 `created`/`modified`。**例外（2026-09 明确）**：单调 `Instant`（非挂钟、不可持久化、不随系统时钟跳变）可用于动画计时——native 侧 caret blink 的 `tick_blink` 即此用途；wasm 侧不使用 `Instant`。

### 4.5 渲染产出 `imaging::record::Scene`，不是 `vello::Scene`

### 4.6 错误显式分层，绝不静默吞

### 4.7 没有 `Format` trait

### 4.8 ID 约定

### 4.9 平台边界：功能内聚核心层，适配器只做绑定

---

## 5. 各 crate 工作要点

### rofd-component

- 唯一入口 `OfdComponent`：`new_native()`/`new_wasm()`（`#[cfg(target_arch = "wasm32")]` 门控，非 feature）、`handle_event`。
- `OfdConfig`：`default_font_bytes`、`page_gap`（默认 20）、`zoom`（初始 PX_PER_MM，`with_zoom` 可覆盖）。
- 场景脏缓存（终态公共 API）：`update_scene()` 按脏标志重合成、`scene() -> &Scene`、`set_viewport_size(f64,f64)`、`mark_scene_dirty`（pub(crate)）。
- 文本编辑：`paste_text`、`delete_annotation`、`tick_blink`（native）、preedit 状态机（组件自持 `Option<PreeditState>`，简化 caret overlay，不做 ghost-document 回流）。

### rofd-xilem-view（as-built 九条契约）

见 §"Native Ofd Widget"。

### rofd-web-view

- `WasmOfd`（wasm-bindgen）+ `WebGpuRenderTarget` + JS 事件桥。工厂 `create_wasm_ofd`。wasm-pack --target web。
- SDK 在 `crates/web-view/sdk/`，入口 `Ofd.init(container, config?)`，发布为 npm 包 `@office-rs/rofd`。

### xilem-app

- 纯 `Xilem::new_simple` 宿主。AppState 为 plain data（命令队列 + file/package 路径 + modified + has_selection + warnings + context_menu），无锁。
- 文件 I/O 落 `src/host/document_io.rs`：`LoadedOfd{document, package, warnings}`、`load_ofd`、`save_ofd`（Some(pkg) 手术刀 / None 全量；原子写）。Save-As 不产生新 PackageHandle。

---

## Native Ofd Widget（masonry/xilem 适配器）

`rofd-xilem-view` 把编辑器嵌为一等 masonry widget。宿主不碰 winit——focus、pointer capture、IME session、clipboard 路由全部经 masonry：

```
ofd(queue) / ofd_with_config(queue, config)    xilem View (ofd_view.rs)
  │ build: OfdWidget::new(config); actions ↔ .on_change/.on_context_menu/… handlers
  │ rebuild: drains `queue` → OfdWidget::with_component (host→widget commands)
  ▼
OfdWidget (masonry Widget)                      owns OfdComponent, pending queue,
  │                                               effective focus, pointer cursor
  │ masonry events → masonry_events.rs → ViewEvent
  │ component callbacks → pending queue → ctx.submit_action (after every touch)
  │ paint: component.update_scene() + painter.replay(scene)
  ▼
OfdComponent (rofd-component)
```

Key contracts:

1. **Host→widget commands**：`OfdCommand = Arc<dyn Fn(&mut OfdComponent) + Send + Sync>`；`OfdCommandQueue = Arc<Mutex<Vec<OfdCommand>>>`，rebuild 时经 `with_component` 排空。任何被处理的回调返回 `MessageResult::Action(())` → 重跑宿主逻辑 → rebuild → 排空。命令必须 `Fn`（按钮重复触发），捕获数据在闭包内 clone。
2. **Widget→host events**：`OfdWidgetAction` 镜像组件回调面；`PointerCursorChanged` 内部消费（驱动 `get_cursor`），绝不 submit。
3. **有效焦点** = widget 键盘焦点 ∧ 窗口焦点；组件出生未聚焦（首次点击前 caret 隐藏）。`Ime::Disabled → FocusLost` 强制提交 preedit，带防御性 regain。
4. **IME**：`accepts_text_input` 自动起停 session；每个触点末尾从 `caret_rect()` 刷新 `set_ime_area`。
5. **剪贴板**：Ctrl+V = `TextEvent::ClipboardPaste` → `paste_text`；Ctrl+C 在 widget 拦截经 `ctx.set_clipboard` 写出。Ctrl+X 在 rofd 为 copy-only（运行期 body 只读、无 selection extent，绝不删除）；宿主菜单无 Cut 项。
6. **Zoom（rofd 有意差异，无镜像）**：组件 viewport 直接乘法缩放（`zoom *= factor`，基线 `PX_PER_MM = 96/25.4`）；ctrl+wheel 发 `ZoomAt{factor, center}`（×1.1 / ×0.9，center = 指针视口位置），钳制 `[PX_PER_MM×0.25, PX_PER_MM×3.0]`。宿主/widget 不保留 f32 zoom 镜像。
7. **Blink**：`on_anim_frame` 在聚焦期间保活动画；500ms 时序归组件 `tick_blink`（单调 Instant）。
8. **Scroll**：wheel 事件 set_handled——滚动与自绘滚动条归组件；无 portal、无 CANVAS_CONTENT_HEIGHT。
9. **无障碍**：`Role::Document`。

If you're tempted to reintroduce a winit-layer bridge、canvas-origin 记账、或每帧 force_repaint 排序——don't；widget 就是整个平台适配器。

---

## 6. 测试约定

- 目标 80% 覆盖，TDD（先红后绿）。
- 手术刀字节保留是核心测试；真实样例测试保持 `#[ignore]`：
  - `cargo test -p xilem-app --test c2_save -- --ignored`
- 渲染用场景结构断言（对象数/类型，非像素）。
- xilem-view 无头 harness（masonry_testing，7 个）：打字、双击选词、IME、滚轮、ctrl+wheel、Ctrl+C、窗口焦点门控。

---

## 7. 依赖钉版（动之前必读）

| crate | 源 | 版本/rev |
| ----- | -- | --------- |
| `xilem` | linebender/xilem git | `271a27a6d4a930f7878d404f9014e3c50a3a9b88` |
| `masonry_testing` | 同上（dev-dep） | 同 rev |
| `imaging` / `imaging_vello` | crates.io | `0.0.1` |
| vello / parley | crates.io | `0.8` |
| wgpu | crates.io | `28` |

Toolchain：`rust-toolchain.toml` 钉 `1.98.1`；新 pin 的包声明 `rust-version = 1.96`。已无 masonry_winit/winit 直接依赖（masonry_winit 内部使用 winit）。

---

## 8. 工作流

superpowers 工作流：spec → plan → TDD → 自审 → 提交，全部 conventional commits。

---

## 9. 常见陷阱

| ✅ Do | ❌ Don't |
| ----- | -------- |
| render 用 imaging Painter API | 在 render 里直接构造 vello::Scene |
| 宿主 set_clock（Instant 仅用于 blink 动画） | 库内调挂钟 |
| editor 只改 .annotations | 在 editor 里改 pages |
| xilem-view 只依赖 component | 往适配器塞 io/业务逻辑 |
| 改 io 后跑手术刀测试 | 只断言模型相等 |

---

## 10. 编码规范

- 设计文档可以保留 WPS 字样，但是代码注释、代码变量、代码文件命名不要存留 WPS 字样。

---

## 11. 参考项目

- **`D:/code/rword`** — Rust + GPU 的文档编辑器库，rofd 本次 masonry/xilem 改造与更名的 as-built 参照：适配器三文件（word_widget/masonry_events/word_view）、宿主（xilem-app + host/document_io）、更名 commit 序列。
- **`D:/code/reditor`** — 更早的 OOXML 编辑器库，rofd 分层骨架的历史来源。
```

- [ ] **Step 2: 根 README（EN）**

把 `README.md` 中：

```ts
import { Editor } from '@office-rs/rofd';

const container = document.getElementById('container') as HTMLElement;

const editor = await Editor.init(container, {
```

替换为：

```ts
import { Ofd } from '@office-rs/rofd';

const container = document.getElementById('container') as HTMLElement;

const ofd = await Ofd.init(container, {
```

把：

```ts
editor.setClock('rofd', Date.now());

editor.loadOfd(bytes);
```

替换为：

```ts
ofd.setClock('rofd', Date.now());

ofd.loadOfd(bytes);
```

- [ ] **Step 3: 根 README.zh-CN**

把 `README.zh-CN.md` 中：

```ts
import { Editor } from '@office-rs/rofd';

const container = document.getElementById('container') as HTMLElement;

const editor = await Editor.init(container, {
```

替换为：

```ts
import { Ofd } from '@office-rs/rofd';

const container = document.getElementById('container') as HTMLElement;

const ofd = await Ofd.init(container, {
```

把：

```ts
editor.setClock('rofd', Date.now());

editor.loadOfd(bytes);
```

替换为：

```ts
ofd.setClock('rofd', Date.now());

ofd.loadOfd(bytes);
```

- [ ] **Step 4: CHANGELOG 顶部新增条目**

在 `CHANGELOG.md` 的标题说明段之后、`## SDK 0.2.0` 之前插入：

```md
## SDK 0.1.6 (@office-rs/rofd)

Rename release: the SDK class and the wasm/Rust interface family move to
their final `Ofd*` names. Ships together with the native masonry/xilem
adapter rewrite and the crate renames (`rofd-xilem-view` / `xilem-app`).

Note: 0.1.6 is chronologically newer than 0.2.0 but sits lower in semver
numbering; it supersedes 0.2.0's API surface entirely.

### Breaking
- **`Editor` class → `Ofd`**: `Editor.init(...)` becomes `Ofd.init(...)`;
  no alias is kept.
- **`WasmEditor` → `WasmOfd`**; factory **`create_wasm_editor` →
  `create_wasm_ofd`**; **`EditorConfig` → `OfdConfig`**.
- **Rust core**: `EditorComponent` → `OfdComponent`, `EditorConfig` →
  `OfdConfig`.
- Crates `rofd-native-view` / `native-app` renamed `rofd-xilem-view` /
  `xilem-app`.

### Changed
- Native adapter is now a masonry `Widget` + xilem `View` (`OfdWidget`,
  `ofd()`/`ofd_with_config()`); the native host is a pure
  `Xilem::new_simple` app with a command queue, toolbar and context-menu
  overlay. Body zoom is component-owned multiplicative zoom (no host
  mirror); Ctrl+X is copy-only.

```

- [ ] **Step 5: 历史文档处理决策**

与 rword as-built 一致：`docs/superpowers/` 与 `tmp/` 下历史 spec/plan **不加横幅、不改内容**；本仓库无根级 IMPLEMENTATION_SUMMARY 类文件，无需横幅。

- [ ] **Step 6: 文档旧名审计**

Run:

```bash
! rg -n "\bEditorComponent\b|\bEditorConfig\b|\bWasmEditor\b|rofd-native-view|rofd_native_view" \
  AGENTS.md README.md README.zh-CN.md CHANGELOG.md
```

Expected: AGENTS/README 零命中。CHANGELOG 允许两类旧名：新条目迁移映射中的 `Editor*`（有意保留），以及 `## SDK 0.2.0` 历史条目内的 `EditorConfig`（编年史准确描述当时 API，不回改，同历史文档规则）。审计意图是"作为当前 API 使用的旧名"。

- [ ] **Step 7: 提交**

```bash
git add -A
git commit -m "docs: 终态更名同步——AGENTS/README/CHANGELOG"
```

---

### Task 6 (C6): 终验 + 全仓旧名零命中审计

**Files:** 无文件改动（纯验证；审计有漏则回到对应任务补 `fix:` 提交）。

- [ ] **Step 1: 全部门禁重跑（终态树，不信"之前绿过"）**

```bash
cargo build --workspace
cargo test --workspace --exclude tauri-app --exclude rofd-web-view
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
cargo check -p rofd-web-view --target wasm32-unknown-unknown
```

- [ ] **Step 2: 前端门禁**

```bash
cd crates/web-view/sdk && npm run build:ts
cd ../..
cd web-app && npm run type-check && npm run build
cd ../..
```

- [ ] **Step 3: 手术刀真实样例（若有 gitignored 样例）**

```bash
cargo test -p xilem-app --test c2_save -- --ignored
```

- [ ] **Step 4: 全仓旧名零命中审计（playbook §5 模式）**

Run:

```bash
grep -rn "rofd-native-view\|rofd_native_view\|EditorComponent\|EditorConfig\|EditorWidget\|EditorView\|EditorCommand\|WasmEditor\|IMEWasmEditor\|useEditor\|injectEditor\|EditorCanvas\|editor_with_config\|create_wasm_editor" \
  --include="*.rs" --include="*.toml" --include="*.ts" --include="*.vue" --include="*.md" --include="*.yml" . \
  | grep -v target | grep -v node_modules | grep -v "/dist/" \
  | grep -v "docs/superpowers" | grep -v "tmp/" | grep -v "^./.superpowers/" \
  | grep -v "CHANGELOG.md"
```

说明：`./.superpowers/` 是 gitignored 的本地 SDD 工作笔记，不属仓库内容；CHANGELOG 整体豁免（新条目映射行 + 0.2.0 历史条目均有意保留旧名，见 Task 5 Step 6）。

Expected: 零输出。作为当前 API 使用的旧名一律修复。

- [ ] **Step 5: sanity 对照（证明审计管道活着）**

Run:

```bash
grep -rn "EditorComponent" docs/superpowers/specs/2026-07-08-ofd-editor-design.md | head -1
```

Expected: 有命中（历史文档保留旧名）。若无命中，说明审计 grep 本身坏了，修好后重跑 Step 4。

- [ ] **Step 6: 若有补漏，提交**

```bash
# 仅当审计发现遗漏时：
git add -A
git commit -m "fix: 补漏更名"
```

---

## C（全部改造）完成判据

- [ ] 全仓作为当前 API 的旧名零命中：EditorComponent/EditorConfig/WasmEditor/create_wasm_editor/rofd-native-view/native-app（历史文档与 CHANGELOG 映射行除外）。
- [ ] SDK 包名/版本：`@office-rs/rofd@0.1.6`，导出 `Ofd`/`OfdConfig`。
- [ ] `cargo run -p xilem-app`（含命令行带参）全流程：工具栏、右键 overlay、Ctrl+S、ctrl+wheel 缩放、IME、CJK；`npm run dev` 对等。
- [ ] Rust 门禁 + wasm check + 前端构建 + 手术刀字节保留全绿。
