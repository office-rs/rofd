import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { Ofd } from "../index";
import { RafStub } from "./helpers";

type WasmSurface = Parameters<typeof Ofd.__testNew>[0];

/** Synthetic wasm surface with spies on the IME/keyboard surface. */
function makeOfd(): {
  ofd: Ofd;
  wasm: Record<string, ReturnType<typeof vi.fn>>;
  canvas: HTMLCanvasElement;
  input: HTMLInputElement;
} {
  const wasm = {
    renderFrame: vi.fn(() => -1),
    handleKeyDown: vi.fn(
      (_k: string, _s: boolean, _c: boolean, _a: boolean, _m: boolean) => {},
    ),
    handleImePreedit: vi.fn((_text: string) => {}),
    handleImeCommit: vi.fn((_text: string) => {}),
    handleFocusGained: vi.fn(() => {}),
    handleFocusLost: vi.fn(() => {}),
    handleMouseDown: vi.fn(() => {}),
    handleMouseUp: vi.fn(() => {}),
    caretRect: vi.fn(() => [10, 20, 2, 16]),
  } as unknown as WasmSurface;
  const canvas = document.createElement("canvas");
  document.body.appendChild(canvas);
  const ofd = Ofd.__testNew(wasm, canvas);
  const input = document.querySelector(
    "input[data-rofd-ime]",
  ) as HTMLInputElement | null;
  if (!input) {
    throw new Error("hidden IME input not created");
  }
  return {
    ofd,
    wasm: wasm as unknown as Record<string, ReturnType<typeof vi.fn>>,
    canvas,
    input,
  };
}

describe("IME composition wiring", () => {
  let raf: RafStub;

  beforeEach(() => {
    vi.useFakeTimers();
    raf = new RafStub();
    raf.install();
  });

  afterEach(() => {
    vi.useRealTimers();
    vi.unstubAllGlobals();
  });

  it("bridges composition events to the wasm IME surface and clears after commit", () => {
    const { ofd, wasm, input } = makeOfd();
    try {
      input.dispatchEvent(
        new CompositionEvent("compositionupdate", { data: "你" }),
      );
      expect(wasm.handleImePreedit).toHaveBeenCalledWith("你");

      input.dispatchEvent(
        new CompositionEvent("compositionend", { data: "你好" }),
      );
      expect(wasm.handleImeCommit).toHaveBeenCalledWith("你好");
      // The preedit MUST be cleared after commit, or the component keeps a
      // stale composition that swallows the next keystroke.
      expect(wasm.handleImePreedit).toHaveBeenCalledWith("");
      // The committed text must not also sit in the input (double-insert on
      // a later compositionend would duplicate it).
      expect(input.value).toBe("");
    } finally {
      ofd.destroy();
    }
  });

  it("keydown during composition never reaches handleKeyDown", () => {
    const { ofd, wasm, input } = makeOfd();
    try {
      const composing = new KeyboardEvent("keydown", { key: "a" });
      Object.defineProperty(composing, "isComposing", { value: true });
      input.dispatchEvent(composing);

      const process = new KeyboardEvent("keydown", { key: "Process" });
      Object.defineProperty(process, "keyCode", { value: 229 });
      input.dispatchEvent(process);

      expect(wasm.handleKeyDown).not.toHaveBeenCalled();

      // Non-composing keys still forward with the full modifier surface.
      input.dispatchEvent(new KeyboardEvent("keydown", { key: "a" }));
      expect(wasm.handleKeyDown).toHaveBeenCalledWith(
        "a",
        false,
        false,
        false,
        false,
      );
    } finally {
      ofd.destroy();
    }
  });

  it("keyboard focus lives on the hidden input, not the canvas", () => {
    const { ofd, wasm, input, canvas } = makeOfd();
    try {
      input.focus();
      expect(wasm.handleFocusGained).toHaveBeenCalled();
      // A pointerdown routes focus to the hidden input (the IME target).
      // jsdom lacks pointer capture - stub it (browsers implement it).
      (
        canvas as unknown as { setPointerCapture: (id: number) => void }
      ).setPointerCapture = () => {};
      canvas.dispatchEvent(
        new PointerEvent("pointerdown", {
          button: 0,
          clientX: 5,
          clientY: 5,
          bubbles: true,
        }),
      );
      expect(document.activeElement).toBe(input);
    } finally {
      ofd.destroy();
    }
  });

  it("destroy removes the hidden input from the DOM", () => {
    const { ofd, input } = makeOfd();
    ofd.destroy();
    expect(document.querySelector("input[data-rofd-ime]")).toBeNull();
    expect(input.isConnected).toBe(false);
  });

  it("cursor-change re-anchoring defers past the wasm borrow", async () => {
    // On the REAL surface the cursor-change callback fires synchronously
    // inside the &mut wasm export that moved the caret; calling caretRect()
    // (another &mut export) right there throws WasmRefCell's BORROW_ERROR.
    // The internal subscription must defer the read to a microtask.
    let captured: (() => void) | undefined;
    const wasm = {
      renderFrame: vi.fn(() => -1),
      handleImeCommit: vi.fn(() => {}),
      handleImePreedit: vi.fn(() => {}),
      caretRect: vi.fn(() => [5, 5, 2, 16]),
      setOnCursorChange: vi.fn((cb: () => void) => {
        captured = cb;
      }),
    } as unknown as WasmSurface;
    const canvas = document.createElement("canvas");
    document.body.appendChild(canvas);
    const ofd = Ofd.__testNew(wasm, canvas);
    try {
      const input = document.querySelector(
        "input[data-rofd-ime]",
      ) as HTMLInputElement;
      expect(captured).toBeTypeOf("function");
      input.focus();
      // The wasm bridge fires the callback synchronously (mid-borrow).
      (captured as () => void)();
      expect(wasm.caretRect).not.toHaveBeenCalled();
      await Promise.resolve();
      await Promise.resolve();
      expect(wasm.caretRect).toHaveBeenCalled();
    } finally {
      ofd.destroy();
    }
  });

  it("hidden input is keyboard reachable (tab order)", () => {
    const { ofd, input } = makeOfd();
    try {
      // The pre-migration canvas was tabIndex=0; the hidden input replaces
      // it in the tab order (a11y: keyboard-only users can re-enter).
      expect(input.tabIndex).toBe(0);
    } finally {
      ofd.destroy();
    }
  });
});
