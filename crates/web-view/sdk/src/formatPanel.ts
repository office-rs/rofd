// Floating annotation text-format panel (font family / size / color) for the
// rofd DOM host. The kernel supplies the target, style, anchor and color/size
// tables; this module owns the DOM, placement and open menus.
//
// Focus rule: the panel must never steal focus from the canvas' hidden IME
// input, so every pointerdown inside it calls preventDefault().

/** Kernel surface the panel reads/writes (the `WasmOfd` raw object). */
export interface FormatPanelHost {
  selectedTextAnnotation(): string | null;
  annotationTextStyle(id: string): string[] | null;
  annotationFormatAnchor(id: string): number[] | null;
  fontFamilies(): string[][];
  textSizes(): (string | number)[][];
  textColorStandard(): string[];
  textColorPalette(): string[];
  setAnnotationFontFamily(id: string, family: string): void;
  setAnnotationFontSize(id: string, sizeMm: number): void;
  setTextAnnotationColor(id: string, hex: string): void;
}

/** Panel footprint in CSS px (the kernel-side anchor margin is 8). */
export const PANEL_W = 240;
export const PANEL_H = 28;
export const ANCHOR_MARGIN = 8;

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
  top = Math.min(Math.max(top, ANCHOR_MARGIN), Math.max(ANCHOR_MARGIN, viewport.h - PANEL_H - ANCHOR_MARGIN));
  const left = Math.min(Math.max(x, ANCHOR_MARGIN), Math.max(ANCHOR_MARGIN, viewport.w - PANEL_W - ANCHOR_MARGIN));
  return { left, top };
}

function sizeLabel(entry: (string | number)[]): string {
  const label = String(entry[0] ?? "");
  return label !== "" ? label : String(entry[1]);
}

/** Display label for a millimetre size: the Chinese size name when it matches,
 * otherwise the point value. */
export function sizeDisplay(sizeMm: number, sizes: (string | number)[][]): string {
  const pt = (sizeMm * 72) / 25.4;
  for (const entry of sizes) {
    if (Math.abs(Number(entry[1]) - pt) < 0.05 && String(entry[0] ?? "") !== "") {
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

  if (getComputedStyle(container).position === "static") {
    container.style.position = "relative";
  }

  const root = document.createElement("div");
  root.className = "rofd-format-panel";
  root.style.cssText =
    "position:absolute;display:none;z-index:10;background:#fff;" +
    "border:1px solid #C9CDD4;border-radius:4px;font:12px/1.2 system-ui,sans-serif;" +
    "color:#333840;user-select:none;";
  // Keep the hidden IME input focused; never let a panel press reach the canvas.
  root.addEventListener("pointerdown", (e) => {
    e.preventDefault();
    e.stopPropagation();
  });

  const row = document.createElement("div");
  row.style.cssText = "display:flex;flex-direction:row;";
  const familyBtn = button("rofd-fp-family", "字体");
  const sizeBtn = button("rofd-fp-size", "字号");
  const colorBtn = button("rofd-fp-color", "A");
  row.append(familyBtn, sizeBtn, colorBtn);

  const menu = document.createElement("div");
  menu.className = "rofd-fp-menu";
  menu.style.cssText = "display:none;max-height:260px;overflow-y:auto;";

  root.append(row, menu);
  container.appendChild(root);

  let open: Menu = null;
  let target: string | null = null;
  let style: string[] | null = null;
  // Menu data is static for the session (font lists change only at init):
  // cache it so a per-frame refresh does not rebuild or re-allocate the menu.
  let familiesCache: string[][] | null = null;
  let sizesCache: (string | number)[][] | null = null;
  let paletteCache: string[] | null = null;
  let menuKey = "";

  function button(cls: string, text: string): HTMLButtonElement {
    const b = document.createElement("button");
    b.className = cls;
    b.type = "button";
    b.textContent = text;
    b.style.cssText =
      "width:80px;height:" + PANEL_H + "px;border:1px solid #E0E0E0;background:#fff;" +
      "cursor:pointer;font:inherit;color:inherit;";
    b.addEventListener("pointerdown", (e) => {
      e.stopPropagation();
      e.preventDefault();
      const next: Menu =
        cls === "rofd-fp-family" ? "family" : cls === "rofd-fp-size" ? "size" : "color";
      open = open === next ? null : next;
      rebuildMenu();
    });
    return b;
  }

  function item(text: string, cls: string, onClick: () => void): HTMLElement {
    const d = document.createElement("div");
    d.className = cls;
    d.textContent = text;
    d.style.cssText = "padding:4px 10px;cursor:pointer;white-space:nowrap;";
    d.addEventListener("click", (e) => {
      e.stopPropagation();
      onClick();
      open = null;
      menuKey = "";
      rebuildMenu();
      refresh();
    });
    return d;
  }

  function rebuildMenu(): void {
    menuKey = `${open ?? ""}|${target ?? ""}`;
    menu.replaceChildren();
    menu.style.display = open === null ? "none" : "block";
    if (open === null) {
      return;
    }
    if (open === "family") {
      familiesCache ??= host.fontFamilies();
      for (const [key, display] of familiesCache) {
        menu.appendChild(
          item(display, "rofd-fp-family-item", () => {
            if (target !== null) {
              host.setAnnotationFontFamily(target, key);
              onMutate?.();
            }
          }),
        );
      }
    } else if (open === "size") {
      sizesCache ??= host.textSizes();
      for (const entry of sizesCache) {
        const mm = (Number(entry[1]) * 25.4) / 72;
        menu.appendChild(
          item(sizeLabel(entry), "rofd-fp-size-item", () => {
            if (target !== null) {
              host.setAnnotationFontSize(target, mm);
              onMutate?.();
            }
          }),
        );
      }
    } else {
      const grid = document.createElement("div");
      grid.style.cssText = "display:grid;grid-template-columns:repeat(10,18px);gap:2px;padding:4px;";
      paletteCache ??= [...host.textColorPalette(), ...host.textColorStandard()];
      for (const hex of paletteCache) {
        grid.appendChild(swatch(hex));
      }
      menu.appendChild(grid);
    }
  }

  function swatch(hex: string): HTMLElement {
    const s = document.createElement("div");
    s.className = "rofd-fp-color-item";
    s.style.cssText = "width:16px;height:16px;border:1px solid #D0D0D0;cursor:pointer;background:" + hex + ";";
    s.addEventListener("click", (e) => {
      e.stopPropagation();
      if (target !== null) {
        host.setTextAnnotationColor(target, hex);
        onMutate?.();
      }
      open = null;
      menuKey = "";
      rebuildMenu();
      refresh();
    });
    return s;
  }

  function refresh(): void {
    const id = host.selectedTextAnnotation();
    const nextStyle = id === null ? null : host.annotationTextStyle(id);
    const anchor = id === null ? null : host.annotationFormatAnchor(id);
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
    if (id === null || nextStyle === null || anchor === null || anchor.length !== 4 || !visible) {
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
    familyBtn.textContent = (style[0] ?? "") !== "" ? style[0] : "字体";
    sizesCache ??= host.textSizes();
    sizeBtn.textContent = sizeDisplay(Number(style[1]), sizesCache);
    colorBtn.style.borderBottom = "3px solid " + (style[2] ?? "#000000");

    const vp = viewport();
    const { left, top } = panelAnchor(
      [anchor[0] * scale, anchor[1] * scale, anchor[2] * scale, anchor[3] * scale],
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
