# Changelog

All notable changes to this project are documented here. Desktop releases
follow the git tags published to GitHub Releases (`vX.Y.Z`); the npm SDK
`@office-rs/rofd` is versioned on its own track, documented under
`SDK X.Y.Z` headings.

## Unreleased

### SDK 0.2.0
- **签章与验签，组件和 SDK 同名对齐**（签章 spec 2026-11-23 §17.11、
  §17.13）：实例上新增 `ofd.verifySignatures(options?)`——验当前打开的文档，
  存没存盘都行，不用先存盘；`ofd.sign(pfx, password, options)` 给当前文档
  签章，签名就地落进去，视图、选区、撤销历史不动，用户保存时才写盘。
  两段式的结果 `apply()` 改为返回 `SignStep { done, next, reports }`，签名
  落地时带回报告（原 `SignatureStep` 改名）；`PreparedSignature` 只能
  `apply` 一次，第二次由 SDK 报错。字节版两段式的类型改名
  `PreparedSignOfd` / `SignOfdStep`。新增纯函数 `newlyInvalidated(before,
  after)`（存盘预检用）。wasm 导出 `signBlocked` 改名 `isSignBlocked`，
  实例新增 `verifySignatures(optionsJson)`。
- **CJK 切片自带 ASCII**（draft §4.7"数字变粗"修正，资产层）：本地
  `file:` 依赖刷新为 fonts 仓库 ≥`8537ced` 重切产物（common-0 预载片并入
  U+0020–007E，+3~14 KB）——中文字族声明的混排数字不再落跨族兜底（字形
  粗细不一致）。商用本机切片须用 ≥`50eb570` 的 slice.py 重切（README）。
- **桌面纯系统字体通道**（draft 2026-10-18，第四通道）：SDK 新增可选
  `systemFonts`（`SystemFontProvider { list, read }`）与 `defaultFamily`——
  宿主注入本机字体面，SDK 按文档声明族**懒加载**注册（`settled` 永不重
  试；manifest 覆盖族永不咨询 provider，一族一通道）。tauri 壳新增
  `list_system_fonts`/`read_system_font` 双命令（启动扫描 `%WINDIR%\Fonts`+
  用户字体目录建索引，mmap + name 表全语言提取，读面=已扫描集合，无白名
  单）；桌面 manifest 集为空（D4：Noto 切片不进桌面，安装包 -24MB），默认
  字体宋体（D5）。wasm 面 `normalizeFontName`（内核归一表单一事实源）。
  预留 `getFontFamilies()`（manifest displayName ∪ 系统名，未来批注字体
  面板数据面；**async**——rofd 形态依赖 ready()，与 draft 描述的同步签名
  有别）。浏览器路径逐字节不变（e2e 零回归）。

### Breaking
- **rofd-dom / rofd-io 的公开面**（签章 spec 2026-11-23 §17.11，经 flw
  点头）：`OfdDocument` 新增 `package: Option<PackageHandle>`——文档自带它
  来自的包；`PackageHandle` / `PkgEntry` / `EntryKind` 从 rofd-io 挪到
  rofd-dom，rofd-io 原名再导出。`LoadReport` 只剩 `document` 和
  `warnings`；`save_ofd` 改为 `save_ofd(&document, &text_layout)`，包从
  文档里取。按字面量构造 `OfdDocument` 的代码要补 `package: None`（和
  `signatures`）。`write_ofd` / `write_ofd_with_text_layout` 不变。
- **裸 `Ofd.init` 零字体请求**：SDK 不再默认从 jsDelivr 拉 Noto（内置
  `DEFAULT_FONTS` 已删除）。宿主需显式传 `fontSlices`（切片按需通道）或
  `fonts`（整字体）；两者都不传则不加载任何字体，文本回退宿主环境字形。

### Added
- **签章线**（签章 spec 2026-11-23）：打开即验签，每条签名一份报告
  （Rust `rofd_sign::verify`；SDK `verifySignatures(bytes, options?)`）；
  保真存盘 `saveOfd`——保留原包、只重写批注条目，`isSaveBlockedBySignature`
  / `isSaveReserveExhausted` / `isSaveFullWriteFallback` 说出存盘做了什么；
  点击落位（`beginSealPlacement` / `cancelSealPlacement` /
  `setOnSealPlaced` / `setOnSealPlacementCancelled`）；用证书文件签章，
  签名就地落进当前打开的文档，用户保存时才写盘。
- **组件是库的一个出口**（签章 spec §17.13，feature `sign`）：
  `OfdComponent::sign(signer, options)`、`sign_pfx(pfx, password,
  options)`、`verify_signatures(&options)`、`sign_refusal()`、两段式
  `prepare_sign` / `finish_sign`；签章要用的类型从
  `rofd_component::signing` 再导出。`seal_pictures()` 不需要 feature。
  SDK 是另一个出口，两边同名提供同样的东西。
- **Web 字体切片按需加载**（spec 2026-10-17 font-slicing）：SDK `Ofd.init`
  新增 `fontSlices`（`FontSlicesConfig { manifests, defaultFamily?,
  includeDefaultNoto? }`；缺失目录的 manifest 静默剔除、降级不阻塞渲染）；
  wasm 面 `collectFontRequirements` / `registerFontSlice(bytes, familyKey,
  sliceId)`；渲染内核字形切片注册与缓存失效。web-app 与 tauri-app 宿主经
  `@office-rs/font-runtime/vite` 接入 Noto 两族 npm 切片，商用六族走本机切片
  `public/fonts-sliced/`（gitignored，详见 `crates/web-app/README.md`）。
  验收：首载实测 7 个切片请求（预算 ≤20、总量 <3MB、无单条 >1MB）；
  120 片注册链性能门 ratio 1.36（阈值 10×）。性能闸门为强制 miss 微基准
  （夹具为 120 只拉丁切片对 CJK 文本，伪 worst case）；真实路径由
  glyph_cache 主导零回归，e2e 实测首载 7 个切片请求在预算内。
- 适应宽度 / 适应高度 / 适应页面缩放模式：`set_zoom_mode` 组件 API、SDK `zoomMode`/`setZoomMode`、web 与 native 工具栏按钮；resize/换文档自动重适配，手动缩放退出；fit 可低于 25%（下限 5%）
- Web 按需渲染：SDK 渲染循环自挂起调度（聚焦静止 ~2 帧/s，失焦/后台标签 0 帧 0 定时器），新增 `pauseRendering()` / `resumeRendering()` 宿主政策层 API（spec 2026-10-13 web-render-on-demand）
- 组件 `needs_frame() -> FrameNeed` 调度真值；`RenderTarget::draw_scene` 返回绘制成败，失败下一 vsync 重试

### Changed
- **状态栏说的是当前文档**（签章 spec 2026-11-23 §17.13，M12 定稿）：验签针对打开的文档，存没存盘都行；"验签"按钮不再去验盘上文件。签章落笔产生的告警经 `on_warning` / `setOnWarning` 交给宿主，不再打到控制台。
- wasm caret blink 从 60Hz 帧累计器改为真实时钟 deadline（`web_time::Instant`）；`renderFrame()` 返回下一帧调度需求（0 / 毫秒 / -1）

## v0.1.6

This is the **masonry/xilem rewrite + final naming** release: the winit
bridge is gone, the native adapter is a standard masonry widget, and the
`Editor*` API family moves to its final `Ofd*` names. It ships together
with SDK 0.1.6 (below). The license changes to Apache-2.0.

### Changed
- **Native adapter rewrite:** the winit event bridge and `EditorApp` are
  deleted. The adapter is now a masonry `Widget` + xilem `View` in three
  files — `OfdWidget`, `masonry_events` (pure translation table), and
  `OfdView` (11 event chains). Focus, pointer capture, IME sessions,
  clipboard shortcuts, and ctrl+wheel zoom are handled inside the widget;
  the host never touches winit.
- **Host rewrite:** the desktop host is a pure `Xilem::new_simple` app
  with an `Arc<dyn Fn(&mut OfdComponent)>` command queue, a toolbar and a
  right-click overlay; file load/save routing and atomic writes live in
  `host/document_io`.
- **Crate renames:** `rofd-native-view` → `rofd-xilem-view`,
  `native-app` → `xilem-app`.
- **API renames:** `EditorComponent` → `OfdComponent`,
  `EditorConfig` → `OfdConfig`, `WasmEditor` → `WasmOfd`,
  `create_wasm_editor` → `create_wasm_ofd`; the SDK class
  `Editor` → `Ofd`. No aliases are kept.

### Added
- **CJK IME support:** preedit state machine (forced commit/cancel/navigation
  guards), a preedit overlay at the caret (Parley shaping, TextBox clipping),
  and a focus-gated blinking caret on both native and web.
- **Clipboard editing:** `paste_text(&str) -> bool` and
  `copy_selection() -> Option<String>` (Ctrl+X is copy-only).
- A README native integration example for `rofd-xilem-view`.

### Fixed
- **Queued save results are reported back:** a failed save no longer
  clears the modified indicator (internal bounded wake, no rerender loop).
- **Save-As** switches the file path only after the save succeeds; failure
  keeps the previous path.
- **Zoom defenses:** non-finite/non-positive inputs are rejected at the
  boundary, and Ctrl + pure horizontal scrolling no longer triggers zoom
  (native and web).
- Scrollbar thumb clamping no longer panics on very small windows; the
  preedit overlay is offset by the TextBox origin; direct annotation
  mutation now invalidates the scene cache.

### License
- Changed from GPL-3.0-or-later to **Apache-2.0**.

### Downloads
Windows installers (pick one):

| File | Installer | Notes |
|---|---|---|
| `rofd_0.1.6_x64-setup.exe` | NSIS | Standard install wizard, recommended |
| `rofd_0.1.6_x64_en-US.msi` | MSI | Suited for enterprise bulk deployment |

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
  - Queued saves report their result back to the host: a failed save no
    longer clears the modified indicator (internal bounded wake).

## SDK 0.2.0 (@office-rs/rofd)

Breaking release of the npm SDK: markup annotations (highlight / underline /
strikeout / squiggly) are no longer creation tools — you select body text
first, then apply.

Note: the Rust crates carry matching workspace-internal changes (the
`rofd-component` `Tool` enum tightening and the removal of
`create_highlight_from_selection`), but they are not published to
crates.io — the published, semver-tracked surface is the npm SDK alone.

### Breaking
- **`createHighlightFromSelection(color)` removed.** Use `applyMarkup(kind)`
  instead. Per-kind colors are configured through the existing
  `setMarkupColor(kind, color)` (and `setHighlightColor` for highlights).
- **`setTool` no longer accepts markup strings.** `highlight`, `underline`,
  `strikeout`, and `squiggly` are no longer tools; these legacy values now
  silently fall back to the text tool, like any unknown value.

### Added
- `applyMarkup(kind: MarkupKind): string | null` — convert the current
  body-text selection into a markup annotation. Returns the new annotation
  id, or `null` when there is no selection. The selection is kept so further
  markups can stack on the same range.
- `hasTextSelection(): boolean` — whether a body-text selection currently
  exists.
- `setOnTextSelectionChange(cb)` / `EditorConfig.onTextSelectionChange` —
  signal fired when the body-text selection appears, changes, or clears.
- `MarkupKind` type export: `'highlight' | 'underline' | 'strikeout' | 'squiggly'`.

### Changed
- Markup annotations are now created by selecting body text with the text
  tool, then clicking a toolbar button (or picking a color). Markup buttons
  disable while no text is selected.

### Fixed
- Host callbacks are now deferred to a microtask before invocation. A handler
  that called back into the editor synchronously (e.g. querying
  `hasTextSelection()` inside `onTextSelectionChange`) re-entered the wasm
  object's borrow, which wasm-bindgen rejects with "recursive use of an
  object detected" — and the error was swallowed, so markup buttons stayed
  permanently disabled after selecting text.

## v0.1.4

This release introduces a **Tauri desktop client** for Windows and restructures
the host applications into first-class workspace crates.

### Added
- **Desktop client (Tauri):** new **rofd** Windows desktop app that wraps the web
  editor inside the system WebView (WebView2), with no CDN dependency — fonts and
  the sample OFD load locally. The frontend reuses `crates/web-app` source
  verbatim; only a native file bridge is injected, and the Rust shell just opens
  the window and registers plugins (depends on no rofd crate). Native open/save
  dialogs via `tauri-plugin-dialog` + `tauri-plugin-fs`. Installer, intermediate
  exe, and installed program are all named **rofd**.
- **Automated release packaging:** on a published GitHub Release,
  `release-tauri.yml` builds the Windows installers on a Windows runner and
  uploads them to the release Assets. Version comes from the git tag.

### Changed
- **fileHost platform file bridge:** file open/save in web-app is now an
  injectable interface, so the same UI runs unchanged in the browser and under
  Tauri.
- Migrated `native-app` and `web-app` from `examples/` to `crates/` as proper
  workspace members; all path references (Cargo, CI, docs) updated accordingly.
- README split into English (`README.md`) and Chinese (`README.zh-CN.md`) with a
  language switcher, documenting how to download, use, and build the Tauri
  desktop client.

### Fixed
- CI: excluded `tauri-app` from the Linux workspace build — it is Windows-only
  and would otherwise fail `glib-sys` on Ubuntu runners. Its build is validated
  on Windows via the release workflow.

### Downloads
Windows installers (pick one):

| File | Installer | Notes |
|---|---|---|
| `rofd_0.1.4_x64-setup.exe` | NSIS | Standard install wizard, recommended |
| `rofd_0.1.4_x64_en-US.msi` | MSI | Suited for enterprise bulk deployment |

The web SDK is published to npm as `@office-rs/rofd@0.1.4`.

**Full changelog:** https://github.com/ravenq/rofd/compare/v0.1.3...v0.1.4
