import { describe, it, expect } from "vitest";
import { createFormatPanel, panelAnchor, sizeDisplay } from "../formatPanel";

function host(overrides: Partial<Record<string, unknown>> = {}) {
  const calls: string[] = [];
  return {
    calls,
    selectedTextAnnotation: () => "7",
    annotationTextStyle: () => ["宋体", "4.233333333333333", "#000000", "0"],
    annotationFormatAnchor: () => [100, 100, 200, 50],
    fontFamilies: () => [
      ["simsun", "宋体"],
      ["simhei", "黑体"],
    ],
    textSizes: () => [
      ["一号", 26],
      ["", 12],
    ],
    textColorStandard: () => ["#000000", "#FF0000"],
    textColorPalette: () => ["#FFFFFF", "#000000"],
    setAnnotationFontFamily: (id: string, f: string) =>
      calls.push(`family:${id}:${f}`),
    setAnnotationFontSize: (id: string, s: number) =>
      calls.push(`size:${id}:${s}`),
    setTextAnnotationColor: (id: string, c: string) =>
      calls.push(`color:${id}:${c}`),
    ...overrides,
  };
}

const vp = () => ({ w: 800, h: 600 });

describe("format panel", () => {
  it("shows above the selected text box with the current values", () => {
    const root = document.createElement("div");
    createFormatPanel(root, host() as never, vp);
    const el = root.querySelector(".rofd-format-panel") as HTMLElement;
    expect(el.style.display).not.toBe("none");
    expect(el.style.top).toBe("64px"); // 100 - 28 - 8
    expect(el.style.left).toBe("100px");
    expect(el.textContent).toContain("宋体");
    expect(el.textContent).toContain("12");
  });

  it("hides when nothing is selected", () => {
    const root = document.createElement("div");
    createFormatPanel(
      root,
      host({ selectedTextAnnotation: () => null }) as never,
      vp,
    );
    const el = root.querySelector(".rofd-format-panel") as HTMLElement;
    expect(el.style.display).toBe("none");
  });

  it("picking a size applies it and keeps the panel open", () => {
    const root = document.createElement("div");
    const h = host();
    createFormatPanel(root, h as never, vp);
    (root.querySelector(".rofd-fp-size") as HTMLElement).dispatchEvent(
      new MouseEvent("pointerdown", { bubbles: true, cancelable: true }),
    );
    const item = [...root.querySelectorAll(".rofd-fp-size-item")].find(
      (n) => n.textContent === "一号",
    ) as HTMLElement;
    item.dispatchEvent(new MouseEvent("click", { bubbles: true }));
    const applied = h.calls.find((c) => c.startsWith("size:7:"));
    expect(applied).toBeDefined();
    expect(Number(applied!.split(":")[2])).toBeCloseTo((26 * 25.4) / 72, 6);
    expect(h.selectedTextAnnotation()).toBe("7");
    expect(
      (root.querySelector(".rofd-format-panel") as HTMLElement).style.display,
    ).not.toBe("none");
  });

  it("picking a family applies its key", () => {
    const root = document.createElement("div");
    const h = host();
    createFormatPanel(root, h as never, vp);
    (root.querySelector(".rofd-fp-family") as HTMLElement).dispatchEvent(
      new MouseEvent("pointerdown", { bubbles: true, cancelable: true }),
    );
    (root.querySelector(".rofd-fp-family-item") as HTMLElement).dispatchEvent(
      new MouseEvent("click", { bubbles: true }),
    );
    expect(h.calls).toContain("family:7:simsun");
  });

  it("pointerdown on the panel does not reach the canvas or steal focus", () => {
    const root = document.createElement("div");
    createFormatPanel(root, host() as never, vp);
    const ev = new MouseEvent("pointerdown", {
      bubbles: true,
      cancelable: true,
    });
    (root.querySelector(".rofd-fp-family") as HTMLElement).dispatchEvent(ev);
    expect(ev.defaultPrevented).toBe(true);
  });

  it("schedules a frame after a mutation", () => {
    const root = document.createElement("div");
    let kicks = 0;
    createFormatPanel(root, host() as never, vp, {}, () => {
      kicks += 1;
    });
    (root.querySelector(".rofd-fp-size") as HTMLElement).dispatchEvent(
      new MouseEvent("pointerdown", { bubbles: true, cancelable: true }),
    );
    const item = [...root.querySelectorAll(".rofd-fp-size-item")].find(
      (n) => n.textContent === "一号",
    ) as HTMLElement;
    item.dispatchEvent(new MouseEvent("click", { bubbles: true }));
    expect(kicks).toBe(1);
  });

  it("keeps the open menu DOM across refreshes (no scroll reset)", () => {
    const root = document.createElement("div");
    const panel = createFormatPanel(root, host() as never, vp);
    (root.querySelector(".rofd-fp-family") as HTMLElement).dispatchEvent(
      new MouseEvent("pointerdown", { bubbles: true, cancelable: true }),
    );
    const first = root.querySelector(".rofd-fp-family-item");
    panel.refresh();
    expect(first).not.toBeNull();
    expect(root.querySelector(".rofd-fp-family-item")).toBe(first);
  });

  it("treats undefined wasm Options as 'no target'", () => {
    // wasm-bindgen maps Rust `Option::None` to `undefined`, not `null`.
    const root = document.createElement("div");
    createFormatPanel(
      root,
      host({ selectedTextAnnotation: () => undefined }) as never,
      vp,
    );
    expect(
      (root.querySelector(".rofd-format-panel") as HTMLElement).style.display,
    ).toBe("none");
  });

  it("treats an undefined style/anchor as 'no target'", () => {
    const root = document.createElement("div");
    const panel = createFormatPanel(
      root,
      host({
        annotationTextStyle: () => undefined,
        annotationFormatAnchor: () => undefined,
      }) as never,
      vp,
    );
    panel.refresh();
    expect(
      (root.querySelector(".rofd-format-panel") as HTMLElement).style.display,
    ).toBe("none");
  });

  it("hides when the box is outside the viewport", () => {
    const root = document.createElement("div");
    createFormatPanel(
      root,
      host({ annotationFormatAnchor: () => [100, 700, 200, 50] }) as never,
      () => ({ w: 800, h: 600 }),
    );
    const el = root.querySelector(".rofd-format-panel") as HTMLElement;
    expect(el.style.display).toBe("none");
  });

  it("anchor flips below near the top and clamps horizontally", () => {
    expect(panelAnchor([10, 10, 200, 50], { w: 800, h: 600 })).toEqual({
      left: 10,
      top: 68,
    });
    expect(panelAnchor([700, 300, 60, 20], { w: 800, h: 600 }).left).toBe(552);
  });

  it("shows the style at the start of the text selection", () => {
    const root = document.createElement("div");
    let styled = ["宋体", "4.23", "#000000", "1"];
    const h = host({ annotationTextStyle: () => styled });
    const panel = createFormatPanel(root, h as never, vp);
    expect(
      (root.querySelector(".rofd-fp-color") as HTMLElement).style.borderBottom,
    ).toContain("rgb(0, 0, 0)");
    styled = ["黑体", "7.06", "#FF0000", "1"]; // selection moved inside the box
    panel.refresh();
    const el = root.querySelector(".rofd-format-panel") as HTMLElement;
    expect(el.textContent).toContain("黑体");
    expect(
      (root.querySelector(".rofd-fp-color") as HTMLElement).style.borderBottom,
    ).toContain("rgb(255, 0, 0)");
  });

  it("size labels prefer the Chinese name and fall back to points", () => {
    const sizes = [
      ["一号", 26],
      ["", 12],
    ];
    expect(sizeDisplay((26 * 25.4) / 72, sizes)).toBe("一号");
    expect(sizeDisplay((13 * 25.4) / 72, sizes)).toBe("13");
  });
});
