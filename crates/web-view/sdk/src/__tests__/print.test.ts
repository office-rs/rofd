// Print surface tests (print spec 2026-10-31 Task 5): the Ofd
// exportPdf/planPages/print wiring, the font-flush gate, FontWatch.pending,
// and the iframe print helper ported from the rword session manager.
//
// Environment notes (jsdom, the SDK's vitest default):
//   - URL.createObjectURL/revokeObjectURL are NOT implemented in jsdom and
//     are replaced by recording stubs per test.
//   - jsdom never fires iframe `load` for blob srcs, so printViaIframe
//     always resolves through its 700ms timer floor here — which doubles as
//     the floor's regression coverage. The onload-before-appendChild
//     ordering is proven at appendChild time instead (spy order check).
//   - assigning frame.src makes jsdom swap in a fresh browsing context, so
//     the child window's focus/print are stubbed AFTER printViaIframe's
//     synchronous prefix (silences jsdom's not-implemented print and makes
//     "the dialog was reached" assertable).
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { Mock } from 'vitest';
import { Ofd } from '../index';
import type { PrintOptions } from '../index';
import { printViaIframe, toBase64 } from '../print';
import { scheduleFontWatch, WATCH_DEBOUNCE_MS } from '../fontFlow';
import type { WasmFontSurface, FontWatch } from '../fontFlow';
import type { FontRuntime } from '@office-rs/font-runtime';
import { RafStub } from './helpers';

type WasmSurface = Parameters<typeof Ofd.__testNew>[0];

type PrintWasm = WasmSurface & {
  exportPdf: Mock;
  planPrintPages: Mock;
  firstPagePaper: Mock;
};

/** Synthetic wasm surface with print spies (the ofd-wiring mock-wasm
 * pattern; renderFrame -1 keeps the scheduler suspended). Spies must be
 * asserted on the ORIGINAL functions — `ofd.wasm` is the kick proxy, whose
 * per-property wrappers are not spies. */
function makeOfd(): {
  ofd: Ofd;
  wasm: PrintWasm;
  mocks: { exportPdf: Mock; planPrintPages: Mock; firstPagePaper: Mock };
} {
  const mocks = {
    exportPdf: vi.fn((): Uint8Array => new Uint8Array([0x25, 0x50, 0x44, 0x46])),
    planPrintPages: vi.fn((): Uint32Array => new Uint32Array([1, 2, 3])),
    firstPagePaper: vi.fn((): string[] => ['A5', 'Landscape']),
  };
  const wasm = {
    renderFrame: vi.fn((): number => -1),
    ...mocks,
  };
  const ofd = Ofd.__testNew(wasm as unknown as WasmSurface, document.createElement('canvas'));
  return { ofd, wasm: (ofd as unknown as { wasm: PrintWasm }).wasm, mocks };
}

/** Privately inject a font watch (the flush-gate seam). */
function setFontWatch(ofd: Ofd, watch: FontWatch): void {
  (ofd as unknown as { fontWatch?: FontWatch }).fontWatch = watch;
}

/** jsdom has no URL.createObjectURL/revokeObjectURL — install recording
 * stubs and return the restore closure. */
function stubObjectUrls(): { created: string[]; revoked: string[]; restore: () => void } {
  const created: string[] = [];
  const revoked: string[] = [];
  const urlStatics = URL as unknown as {
    createObjectURL?: (blob: Blob) => string;
    revokeObjectURL?: (url: string) => void;
  };
  const origCreate = urlStatics.createObjectURL;
  const origRevoke = urlStatics.revokeObjectURL;
  urlStatics.createObjectURL = (): string => {
    const url = `blob:mock-${created.length + 1}`;
    created.push(url);
    return url;
  };
  urlStatics.revokeObjectURL = (url: string): void => {
    revoked.push(url);
  };
  const restore = (): void => {
    if (origCreate !== undefined) {
      urlStatics.createObjectURL = origCreate;
    } else {
      delete urlStatics.createObjectURL;
    }
    if (origRevoke !== undefined) {
      urlStatics.revokeObjectURL = origRevoke;
    } else {
      delete urlStatics.revokeObjectURL;
    }
  };
  return { created, revoked, restore };
}

describe('Ofd print surface', () => {
  beforeEach(() => {
    // Absorb the proxy kicks fired by every wasm call (no frames run).
    new RafStub().install();
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it('exportPdf awaits pending font loads before calling wasm', async () => {
    const { ofd, mocks } = makeOfd();
    try {
      let release!: () => void;
      const gate = new Promise<void>((resolve) => {
        release = resolve;
      });
      const pending = vi.fn(() => gate);
      setFontWatch(ofd, { kick: vi.fn(), cancel: vi.fn(), pending });

      const exported = ofd.exportPdf();
      // Gate unresolved: the wasm call stays parked behind `await pending()`
      // no matter how many microtasks flush.
      await Promise.resolve();
      await Promise.resolve();
      expect(pending).toHaveBeenCalledTimes(1);
      expect(mocks.exportPdf).not.toHaveBeenCalled();

      release();
      expect(await exported).toEqual(new Uint8Array([0x25, 0x50, 0x44, 0x46]));
      expect(mocks.exportPdf).toHaveBeenCalledTimes(1);
    } finally {
      ofd.destroy();
    }
  });

  it('exportPdf forwards options and defaults', async () => {
    const { ofd, mocks } = makeOfd();
    try {
      // No options: every flag passes undefined so the wasm defaults apply
      // (clean-copy preset). No font watch configured either — export goes
      // straight through with no gate.
      await ofd.exportPdf();
      expect(mocks.exportPdf).toHaveBeenCalledWith(undefined, undefined, undefined, undefined);

      const options: PrintOptions = { watermark: true, annotations: true, seal: false, pages: [1, 3] };
      await ofd.exportPdf(options);
      expect(mocks.exportPdf).toHaveBeenLastCalledWith(true, true, false, [1, 3]);
    } finally {
      ofd.destroy();
    }
  });

  it('planPages forwards range/parity/currentPage and flattens the result', async () => {
    const { ofd, mocks } = makeOfd();
    try {
      const pages = await ofd.planPages({ kind: 'Custom', spec: '1-3,5' }, 'Odd', 2);
      expect(mocks.planPrintPages).toHaveBeenCalledWith('Custom', '1-3,5', 'Odd', 2);
      expect(pages).toEqual([1, 2, 3]);
    } finally {
      ofd.destroy();
    }
  });

  it('firstPagePaper passes the wasm paper/orientation defaults through', () => {
    const { ofd, mocks } = makeOfd();
    try {
      // Sync passthrough (metadata only, no font gate): the print dialog
      // reads the document's first-page paper + orientation as defaults.
      expect(ofd.firstPagePaper()).toEqual(['A5', 'Landscape']);
      expect(mocks.firstPagePaper).toHaveBeenCalledTimes(1);
    } finally {
      ofd.destroy();
    }
  });
});

describe('printViaIframe', () => {
  let urls: ReturnType<typeof stubObjectUrls>;

  beforeEach(() => {
    urls = stubObjectUrls();
  });

  afterEach(() => {
    urls.restore();
    vi.restoreAllMocks();
    for (const frame of document.querySelectorAll('iframe')) {
      frame.remove();
    }
  });

  it('printViaIframe keeps frame alive after print and cleans previous pair', async () => {
    vi.useFakeTimers();
    try {
      const order: string[] = [];
      const frames: HTMLIFrameElement[] = [];
      let printCalls = 0;
      const realAppend = document.body.appendChild.bind(document.body);
      vi.spyOn(document.body, 'appendChild').mockImplementation((node: Node) => {
        const frame = node as HTMLIFrameElement;
        if (frame.tagName === 'IFRAME') {
          frames.push(frame);
          // Pitfall 1 (order proof): onload must already be assigned here —
          // appending first would miss about:blank's synchronous load.
          order.push(frame.onload !== null ? 'onload-assigned' : 'onload-missing');
        }
        return realAppend(node);
      });
      // Stub the child window AFTER printViaIframe's synchronous prefix:
      // assigning frame.src makes jsdom swap in a fresh browsing context,
      // so a stub installed at append time would be orphaned.
      const stubDialog = (frame: HTMLIFrameElement): void => {
        const win = frame.contentWindow as unknown as
          | { focus: () => void; print: () => void }
          | null;
        if (win) {
          win.print = (): void => {
            printCalls += 1;
          };
          win.focus = (): void => {};
        }
      };

      const first = printViaIframe(new Uint8Array([1]));
      stubDialog(frames[0]);
      await vi.runAllTimersAsync();
      await first;

      expect(order).toEqual(['onload-assigned']);
      expect(frames).toHaveLength(1);
      // Pitfall 3: after print resolves the frame stays mounted and the
      // blob URL stays live — the preview renders from the frame later.
      expect(document.body.contains(frames[0])).toBe(true);
      expect(urls.revoked).toEqual([]);
      expect(printCalls).toBe(1);

      const second = printViaIframe(new Uint8Array([2]));
      stubDialog(frames[1]);
      await vi.runAllTimersAsync();
      await second;

      expect(order).toEqual(['onload-assigned', 'onload-assigned']);
      expect(frames).toHaveLength(2);
      // Cleanup happens at the START of the next call: the previous pair is
      // removed/revoked exactly once, the current pair stays alive.
      expect(document.body.contains(frames[0])).toBe(false);
      expect(document.body.contains(frames[1])).toBe(true);
      expect(urls.revoked).toEqual([urls.created[0]]);
      expect(printCalls).toBe(2);
    } finally {
      vi.useRealTimers();
    }
  });
});

describe('Ofd.print', () => {
  it('print pipes exportPdf bytes through the iframe', async () => {
    vi.useFakeTimers();
    const urls = stubObjectUrls();
    const { ofd, mocks } = makeOfd();
    try {
      const job = ofd.print();
      // Let the exportPdf microtasks settle: printViaIframe's synchronous
      // prefix (blob URL + frame mount + src) runs right after the wasm
      // export, so by now the frame's final browsing context exists.
      await vi.advanceTimersByTimeAsync(0);
      const frame = document.querySelector('iframe') as HTMLIFrameElement | null;
      expect(frame).not.toBeNull();
      let printCalls = 0;
      const win = (frame as HTMLIFrameElement).contentWindow as unknown as
        | { focus: () => void; print: () => void }
        | null;
      if (win) {
        win.print = (): void => {
          printCalls += 1;
        };
        win.focus = (): void => {};
      }
      await vi.runAllTimersAsync();
      await job;
      expect(mocks.exportPdf).toHaveBeenCalledTimes(1);
      // The exported bytes reached the browser dialog.
      expect(printCalls).toBe(1);
      // Frame mounted and kept alive after the dialog (no early cleanup).
      expect(document.body.contains(frame as HTMLIFrameElement)).toBe(true);
    } finally {
      ofd.destroy();
      urls.restore();
      for (const frame of document.querySelectorAll('iframe')) {
        frame.remove();
      }
      vi.useRealTimers();
    }
  });
});

describe('toBase64', () => {
  it('toBase64 matches btoa on chunked input', () => {
    const lcg = (n: number, seed: number): Uint8Array => {
      const out = new Uint8Array(n);
      let s = seed >>> 0;
      for (let i = 0; i < n; i += 1) {
        s = (Math.imul(s, 1664525) + 1013904223) >>> 0;
        out[i] = s & 0xff;
      }
      return out;
    };
    const reference = (bytes: Uint8Array): string => {
      let bin = '';
      for (let i = 0; i < bytes.length; i += 0x8000) {
        bin += String.fromCharCode(...bytes.subarray(i, i + 0x8000));
      }
      return btoa(bin);
    };
    // Brief case: 4096 pseudo-random bytes.
    expect(toBase64(lcg(4096, 42))).toBe(reference(lcg(4096, 42)));
    // Chunked path: >0x8000 forces at least two 32KiB chunks.
    const big = lcg(0x8000 + 123, 7);
    expect(toBase64(big)).toBe(reference(big));
    // Empty edge.
    expect(toBase64(new Uint8Array(0))).toBe('');
  });
});

describe('FontWatch.pending', () => {
  function fakeWasm(): WasmFontSurface {
    return {
      collectFontRequirements: () => ({ families: [], codepoints: [] }),
      registerFontSlice: (): boolean => true,
    };
  }

  function fakeRuntime(): FontRuntime {
    return {
      ready: async () => ({ families: {} }),
      selectInitial: () => ({ families: [] }),
      watchText: () => ({ families: [] }),
      ensure: async function* () {
        // No slices to yield.
      },
    } as unknown as FontRuntime;
  }

  it('pending resolves immediately when idle', async () => {
    const watch = scheduleFontWatch(fakeWasm(), fakeRuntime(), {});
    let settled = false;
    void watch.pending().then(() => {
      settled = true;
    });
    await Promise.resolve();
    // No timer advance happened: idle means already resolved.
    expect(settled).toBe(true);
    watch.cancel();
  });

  it('pending settles with the kicked round and never hangs after cancel', async () => {
    vi.useFakeTimers();
    try {
      const watch = scheduleFontWatch(fakeWasm(), fakeRuntime(), {});
      watch.kick();
      let settled = false;
      const round = watch.pending();
      void round.then(() => {
        settled = true;
      });
      await vi.advanceTimersByTimeAsync(WATCH_DEBOUNCE_MS - 1);
      // Debounce still armed: the round must not be settled yet.
      expect(settled).toBe(false);
      await vi.advanceTimersByTimeAsync(1);
      await round;
      expect(settled).toBe(true);

      // Round over: idle again resolves immediately.
      let idleSettled = false;
      void watch.pending().then(() => {
        idleSettled = true;
      });
      await Promise.resolve();
      expect(idleSettled).toBe(true);

      // A cancelled watch must not leave pending() waiters hanging.
      watch.kick();
      const cancelledRound = watch.pending();
      watch.cancel();
      await cancelledRound;
    } finally {
      vi.useRealTimers();
    }
  });
});
