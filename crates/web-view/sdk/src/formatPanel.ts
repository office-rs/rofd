// Floating annotation text-format panel (font family / size / color) for the
// rofd DOM host. The kernel supplies the target, style, anchor and color/size
// tables; this module owns the DOM, placement and open menus.
//
// Visual language follows the reference editor's compact ribbon popover:
// borderless toolbar buttons, a floating card with a soft shadow, and a
// sectioned color menu (自动 / 主题颜色 / 标准色 / actions).
//
// Focus rule: the panel must never steal focus from the canvas' hidden IME
// input, so every pointerdown inside it calls preventDefault().

/** Kernel surface the panel reads/writes (the `WasmOfd` raw object). */
// wasm-bindgen maps a Rust `Option::None` to `undefined` (not `null`), so every
// Option-returning export is typed with both and normalized below.
export interface FormatPanelHost {
  selectedTextAnnotation(): string | null | undefined;
  annotationTextStyle(id: string): string[] | null | undefined;
  annotationFormatAnchor(id: string): number[] | null | undefined;
  fontFamilies(): string[][];
  textSizes(): (string | number)[][];
  textColorStandard(): string[];
  textColorPalette(): string[];
  setAnnotationFontFamily(id: string, family: string): void;
  setAnnotationFontSize(id: string, sizeMm: number): void;
  setTextAnnotationColor(id: string, hex: string): void;
}

/** Panel footprint in CSS px (the kernel-side anchor margin is 8). */
export const PANEL_W = 254;
export const PANEL_H = 36;
export const ANCHOR_MARGIN = 8;

/** WPS-like theme grid: 10 columns per row. */
const COLOR_COLUMNS = 10;
/** Width of the sectioned color menu (10 columns x 20px + gaps + padding). */
const COLOR_MENU_W = 268;

type Menu = "family" | "size" | "color" | null;

/** Kernel viewport -> CSS px conversion (DPR + canvas offset). */
export interface PanelPlacement {
  scale?: number;
  offsetX?: number;
  offsetY?: number;
}

/** Above the box; flip below when the top has no room; clamp into the viewport.
 * Same rule as `rofd_render::format_panel_anchor`. */
export function panelAnchor(
  anchor: number[],
  viewport: { w: number; h: number },
): { left: number; top: number } {
  const [x, y, , h] = anchor;
  let top = y - PANEL_H - ANCHOR_MARGIN;
  if (top < ANCHOR_MARGIN) {
    top = y + h + ANCHOR_MARGIN;
  }
  top = Math.min(
    Math.max(top, ANCHOR_MARGIN),
    Math.max(ANCHOR_MARGIN, viewport.h - PANEL_H - ANCHOR_MARGIN),
  );
  const left = Math.min(
    Math.max(x, ANCHOR_MARGIN),
    Math.max(ANCHOR_MARGIN, viewport.w - PANEL_W - ANCHOR_MARGIN),
  );
  return { left, top };
}

function sizeLabel(entry: (string | number)[]): string {
  const label = String(entry[0] ?? "");
  return label !== "" ? label : String(entry[1]);
}

/** Display label for a millimetre size: the Chinese size name when it matches,
 * otherwise the point value. */
export function sizeDisplay(
  sizeMm: number,
  sizes: (string | number)[][],
): string {
  const pt = (sizeMm * 72) / 25.4;
  for (const entry of sizes) {
    if (
      Math.abs(Number(entry[1]) - pt) < 0.05 &&
      String(entry[0] ?? "") !== ""
    ) {
      return String(entry[0]);
    }
  }
  return Number.isInteger(pt) ? String(Math.round(pt)) : pt.toFixed(1);
}

export interface FormatPanel {
  /** Re-read the kernel: show/hide, reposition, refresh labels and the menu. */
  refresh(): void;
  destroy(): void;
}

const STYLE_ID = "rofd-format-panel-style";
const STYLE_TEXT = `
.rofd-format-panel{position:absolute;display:none;z-index:10;background:#fff;
  border:1px solid #E4E6EB;border-radius:6px;box-shadow:0 8px 24px rgba(31,41,55,.16);
  font:12px/1.4 system-ui,-apple-system,"Segoe UI",sans-serif;color:#333840;
  user-select:none;box-sizing:border-box}
.rofd-fp-row{display:flex;align-items:center;gap:2px;padding:3px}
.rofd-fp-btn{position:relative;height:28px;border:0;background:transparent;border-radius:4px;
  cursor:pointer;color:inherit;font:inherit;display:flex;align-items:center;padding:0 8px}
.rofd-fp-btn:hover,.rofd-fp-btn[aria-expanded="true"]{background:#F2F3F5}
.rofd-fp-family{width:132px;justify-content:space-between}
.rofd-fp-family .rofd-fp-label{overflow:hidden;text-overflow:ellipsis;white-space:nowrap}
.rofd-fp-size{width:68px;justify-content:space-between}
.rofd-fp-color{width:36px;justify-content:center}
.rofd-fp-color .rofd-fp-a{font-size:14px;font-weight:600;line-height:1;transform:translateY(-1px)}
.rofd-fp-color .rofd-fp-bar{position:absolute;left:8px;right:8px;bottom:4px;height:3px;
  border-radius:2px;background:#000}
.rofd-fp-caret{width:0;height:0;border-left:4px solid transparent;border-right:4px solid transparent;
  border-top:5px solid #7A7F87;margin-left:6px;flex:0 0 auto}
.rofd-fp-menu{position:absolute;top:36px;display:none;background:#fff;border:1px solid #E4E6EB;
  border-radius:6px;box-shadow:0 8px 24px rgba(31,41,55,.16);padding:4px;z-index:11;
  max-height:320px;overflow-x:hidden;overflow-y:auto;box-sizing:border-box}
.rofd-fp-item{height:28px;line-height:28px;padding:0 10px;border-radius:4px;
  white-space:nowrap;cursor:pointer;overflow:hidden;text-overflow:ellipsis}
.rofd-fp-item:hover{background:#F2F3F5}
.rofd-fp-item.rofd-fp-active{background:#EDF4FF;color:#1F6FEB}
.rofd-fp-color-panel{width:268px;padding:6px 8px 8px}
.rofd-fp-section{font-size:12px;color:#8A8F99;margin:8px 2px 6px}
.rofd-fp-grid{display:grid;grid-template-columns:repeat(${COLOR_COLUMNS},20px);gap:4px;padding:0 2px}
.rofd-fp-swatch{width:20px;height:20px;border-radius:3px;border:1px solid rgba(0,0,0,.08);
  cursor:pointer;box-sizing:border-box}
.rofd-fp-swatch:hover{outline:2px solid #1F6FEB;outline-offset:1px}
.rofd-fp-auto{display:flex;align-items:center;gap:8px;height:28px;padding:0 4px;
  border-radius:4px;cursor:pointer}
.rofd-fp-auto:hover{background:#F2F3F5}
.rofd-fp-auto-swatch{width:20px;height:20px;background:#000;border-radius:3px;
  border:1px solid rgba(0,0,0,.08);box-sizing:border-box}
.rofd-fp-sep{height:1px;background:#EDEFF2;margin:8px 2px}
.rofd-fp-action{display:flex;align-items:center;gap:8px;height:30px;padding:0 6px;
  border-radius:4px;cursor:pointer}
.rofd-fp-action:hover{background:#F2F3F5}
.rofd-fp-action.rofd-fp-disabled{opacity:.45;cursor:default;pointer-events:none}
.rofd-fp-action svg{width:16px;height:16px;flex:0 0 auto;stroke:#333840;fill:none;
  stroke-width:1.4;stroke-linecap:round;stroke-linejoin:round}
.rofd-fp-input{position:absolute;left:0;top:0;width:1px;height:1px;opacity:0;pointer-events:none}
`;

/** Inject the panel stylesheet once per document. */
function ensureStyles(): void {
  const doc = document;
  if (doc.getElementById(STYLE_ID)) {
    return;
  }
  const style = doc.createElement("style");
  style.id = STYLE_ID;
  style.textContent = STYLE_TEXT;
  doc.head.appendChild(style);
}

const ICON_PALETTE =
  '<svg viewBox="0 0 16 16"><circle cx="8" cy="8" r="6"/><circle cx="8" cy="8" r="2"/></svg>';
const ICON_DROPPER =
  '<svg viewBox="0 0 16 16"><path d="M10.5 2.5a1.8 1.8 0 0 1 2.5 2.5l-1 1 .8.8-1.2 1.2-.8-.8-4.6 4.6-2.7.7.7-2.7 4.6-4.6-.8-.8L9.2 3.8l.8.8z"/></svg>';

export function createFormatPanel(
  container: HTMLElement,
  host: FormatPanelHost,
  viewport: () => { w: number; h: number },
  placement: PanelPlacement = {},
  /** Called after a mutation so the host can schedule a render frame. */
  onMutate?: () => void,
): FormatPanel {
  // Version skew guard: an older wasm surface has no panel exports yet
  // (dist/ is built separately); degrade to a no-op panel instead of throwing
  // from init.
  if (typeof host.selectedTextAnnotation !== "function") {
    return { refresh() {}, destroy() {} };
  }

  const scale = placement.scale ?? 1;
  const offsetX = placement.offsetX ?? 0;
  const offsetY = placement.offsetY ?? 0;

  ensureStyles();
  if (getComputedStyle(container).position === "static") {
    container.style.position = "relative";
  }

  const root = document.createElement("div");
  root.className = "rofd-format-panel";
  root.style.width = `${PANEL_W}px`;
  root.style.height = `${PANEL_H}px`;
  // Keep the hidden IME input focused; never let a panel press reach the canvas.
  root.addEventListener("pointerdown", (e) => {
    e.preventDefault();
    e.stopPropagation();
  });

  const row = document.createElement("div");
  row.className = "rofd-fp-row";

  const familyBtn = button("rofd-fp-family", "");
  const familyLabel = el("span", "rofd-fp-label", "字体");
  familyBtn.append(familyLabel, caret());
  const sizeBtn = button("rofd-fp-size", "");
  const sizeLabelEl = el("span", "rofd-fp-label", "字号");
  sizeBtn.append(sizeLabelEl, caret());
  const colorBtn = button("rofd-fp-color", "");
  colorBtn.appendChild(el("span", "rofd-fp-a", "A"));
  const bar = el("span", "rofd-fp-bar", "");
  colorBtn.appendChild(bar);
  row.append(familyBtn, sizeBtn, colorBtn);

  const menu = document.createElement("div");
  menu.className = "rofd-fp-menu";

  const colorInput = document.createElement("input");
  colorInput.type = "color";
  colorInput.className = "rofd-fp-input";
  colorInput.tabIndex = -1;

  root.append(row, menu, colorInput);
  container.appendChild(root);

  let open: Menu = null;
  let target: string | null = null;
  let style: string[] | null = null;
  // Menu data is static for the session (font lists change only at init):
  // cache it so a per-frame refresh does not rebuild or re-allocate the menu.
  let sizesCache: (string | number)[][] | null = null;
  let paletteCache: string[] | null = null;
  let standardCache: string[] | null = null;
  let menuKey = "";

  function el(tag: string, cls: string, text: string): HTMLElement {
    const node = document.createElement(tag);
    node.className = cls;
    if (text !== "") {
      node.textContent = text;
    }
    return node;
  }

  function caret(): HTMLElement {
    return el("span", "rofd-fp-caret", "");
  }

  function button(cls: string, text: string): HTMLButtonElement {
    const b = document.createElement("button");
    b.className = `rofd-fp-btn ${cls}`;
    b.type = "button";
    b.textContent = text;
    b.addEventListener("pointerdown", (e) => {
      e.stopPropagation();
      e.preventDefault();
      const next: Menu =
        cls === "rofd-fp-family"
          ? "family"
          : cls === "rofd-fp-size"
            ? "size"
            : "color";
      open = open === next ? null : next;
      rebuildMenu();
    });
    return b;
  }

  function closeMenu(): void {
    open = null;
    menuKey = "";
    rebuildMenu();
  }

  function applyColor(hex: string): void {
    if (target === null) {
      return;
    }
    host.setTextAnnotationColor(target, hex);
    onMutate?.();
    closeMenu();
    refresh();
  }

  function item(
    label: string,
    cls: string,
    active: boolean,
    onClick: () => void,
  ): HTMLElement {
    const node = el("div", cls, label);
    if (active) {
      node.classList.add("rofd-fp-active");
    }
    node.addEventListener("click", (e) => {
      e.stopPropagation();
      onClick();
      closeMenu();
      refresh();
    });
    return node;
  }

  function itemsMenu(
    width: number,
    entries: { label: string; active: boolean; pick: () => void }[],
    cls: string,
  ): void {
    menu.className = "rofd-fp-menu";
    menu.style.width = `${width}px`;
    for (const entry of entries) {
      menu.appendChild(
        item(entry.label, `rofd-fp-item ${cls}`, entry.active, entry.pick),
      );
    }
  }

  function section(title: string): HTMLElement {
    return el("div", "rofd-fp-section", title);
  }

  function grid(colors: string[]): HTMLElement {
    const box = el("div", "rofd-fp-grid", "");
    for (const hex of colors) {
      box.appendChild(swatch(hex));
    }
    return box;
  }

  function swatch(hex: string): HTMLElement {
    const s = el("div", "rofd-fp-swatch rofd-fp-color-item", "");
    s.style.background = hex;
    s.title = hex;
    s.addEventListener("click", (e) => {
      e.stopPropagation();
      applyColor(hex);
    });
    return s;
  }

  function action(
    label: string,
    icon: string,
    onClick: () => void,
  ): HTMLElement {
    const row = el("div", "rofd-fp-action", "");
    const glyph = el("span", "rofd-fp-action-icon", "");
    glyph.innerHTML = icon;
    row.append(glyph, el("span", "", label));
    row.addEventListener("click", (e) => {
      e.stopPropagation();
      onClick();
    });
    return row;
  }

  function colorMenu(): void {
    menu.className = "rofd-fp-menu rofd-fp-color-panel";
    // Reset the inline width a list menu may have left behind, then size the
    // sectioned palette explicitly (the 10-column grid must not be squeezed).
    menu.style.width = `${COLOR_MENU_W}px`;
    menu.style.maxHeight = "420px";
    paletteCache ??= host.textColorPalette();
    standardCache ??= host.textColorStandard();

    const auto = el("div", "rofd-fp-auto", "");
    auto.append(el("span", "rofd-fp-auto-swatch", ""), el("span", "", "自动"));
    auto.addEventListener("click", (e) => {
      e.stopPropagation();
      applyColor("#000000");
    });
    menu.append(
      auto,
      section("主题颜色"),
      grid(paletteCache),
      section("标准色"),
      grid(standardCache),
    );
    menu.append(el("div", "rofd-fp-sep", ""));

    menu.appendChild(
      action("其他颜色(M)...", ICON_PALETTE, () => {
        if (target !== null) {
          colorInput.value = style?.[2] ?? "#000000";
          colorInput.click();
        }
      }),
    );
    const eye = action("取色器(E)", ICON_DROPPER, () => {
      const Ctor = (
        window as unknown as {
          EyeDropper?: new () => { open(): Promise<{ sRGBHex: string }> };
        }
      ).EyeDropper;
      if (!Ctor) {
        return;
      }
      new Ctor()
        .open()
        .then((result) => applyColor(result.sRGBHex))
        .catch(() => {});
    });
    if (!("EyeDropper" in window)) {
      eye.classList.add("rofd-fp-disabled");
    }
    menu.appendChild(eye);
  }

  function rebuildMenu(): void {
    menuKey = `${open ?? ""}|${target ?? ""}`;
    menu.replaceChildren();
    menu.className = "rofd-fp-menu";
    menu.style.width = "";
    menu.style.maxHeight = "";
    menu.style.display = open === null ? "none" : "block";
    const left =
      open === "family"
        ? familyBtn.offsetLeft
        : open === "size"
          ? sizeBtn.offsetLeft
          : 0;
    menu.style.left = `${left}px`;
    familyBtn.setAttribute("aria-expanded", String(open === "family"));
    sizeBtn.setAttribute("aria-expanded", String(open === "size"));
    colorBtn.setAttribute("aria-expanded", String(open === "color"));
    if (open === null) {
      return;
    }
    if (open === "family") {
      itemsMenu(
        160,
        host.fontFamilies().map(([key, display]) => ({
          label: display,
          active: (style?.[0] ?? "") === display,
          pick: () => {
            if (target !== null) {
              host.setAnnotationFontFamily(target, key);
              onMutate?.();
            }
          },
        })),
        "rofd-fp-family-item",
      );
    } else if (open === "size") {
      sizesCache ??= host.textSizes();
      const current = style ? sizeDisplay(Number(style[1]), sizesCache) : "";
      itemsMenu(
        92,
        sizesCache.map((entry) => ({
          label: sizeLabel(entry),
          active: sizeLabel(entry) === current,
          pick: () => {
            if (target !== null) {
              host.setAnnotationFontSize(
                target,
                (Number(entry[1]) * 25.4) / 72,
              );
              onMutate?.();
            }
          },
        })),
        "rofd-fp-size-item",
      );
    } else {
      colorMenu();
    }
  }

  colorInput.addEventListener("change", () => {
    applyColor(colorInput.value);
  });

  function refresh(): void {
    const id = host.selectedTextAnnotation() ?? null;
    const nextStyle =
      id === null ? null : (host.annotationTextStyle(id) ?? null);
    const anchor =
      id === null ? null : (host.annotationFormatAnchor(id) ?? null);
    // Visibility is judged in kernel (device) space: the viewport callback is
    // CSS px, so scale it back.
    const deviceViewport = {
      w: viewport().w / scale,
      h: viewport().h / scale,
    };
    const visible =
      anchor !== null &&
      anchor.length === 4 &&
      anchor[0] + anchor[2] > 0 &&
      anchor[1] + anchor[3] > 0 &&
      anchor[0] < deviceViewport.w &&
      anchor[1] < deviceViewport.h;
    if (
      id === null ||
      nextStyle === null ||
      anchor === null ||
      anchor.length !== 4 ||
      !visible
    ) {
      target = null;
      style = null;
      open = null;
      menuKey = "";
      menu.style.display = "none";
      root.style.display = "none";
      return;
    }
    target = id;
    style = nextStyle;
    familyLabel.textContent = (style[0] ?? "") !== "" ? style[0] : "字体";
    sizesCache ??= host.textSizes();
    sizeLabelEl.textContent = sizeDisplay(Number(style[1]), sizesCache);
    bar.style.background = style[2] ?? "#000000";

    const vp = viewport();
    const { left, top } = panelAnchor(
      [
        anchor[0] * scale,
        anchor[1] * scale,
        anchor[2] * scale,
        anchor[3] * scale,
      ],
      vp,
    );
    root.style.left = `${Math.round(left + offsetX)}px`;
    root.style.top = `${Math.round(top + offsetY)}px`;
    root.style.display = "block";
    // Rebuilding resets the menu's scroll position and re-allocates the list:
    // only do it when what the menu shows actually changed.
    const nextMenuKey = `${open ?? ""}|${id}`;
    if (nextMenuKey !== menuKey) {
      menuKey = nextMenuKey;
      rebuildMenu();
    }
  }

  refresh();

  return {
    refresh,
    destroy() {
      if (root.parentNode) {
        root.parentNode.removeChild(root);
      }
    },
  };
}
