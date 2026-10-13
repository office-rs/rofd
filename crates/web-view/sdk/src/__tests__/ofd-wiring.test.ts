import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { Ofd } from '../index';
import { RafStub } from './helpers';

type WasmSurface = Parameters<typeof Ofd.__testNew>[0];

/** Synthetic wasm surface: successive renderFrame() values from `nexts`
 * (exhausted -> -1). Only what the wiring touches is implemented. */
function makeOfd(nexts: number[]): { ofd: Ofd; wasm: WasmSurface } {
  const queue = [...nexts];
  let renders = 0;
  const wasm = {
    renderFrame(): number {
      renders += 1;
      return queue.length > 0 ? (queue.shift() as number) : -1;
    },
    handleKeyDown(): void {},
  } as unknown as WasmSurface;
  const ofd = Ofd.__testNew(wasm, document.createElement('canvas'));
  return { ofd, wasm: (ofd as unknown as { wasm: WasmSurface }).wasm };
}

/** The proxy-wrapped wasm surface the Ofd actually holds. */
function proxyOf(ofd: Ofd): WasmSurface {
  return (ofd as unknown as { wasm: WasmSurface }).wasm;
}

describe('Ofd on-demand wiring', () => {
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

  it('suspends after -1; a wasm method call re-arms exactly one rAF', () => {
    const { ofd } = makeOfd([-1]);
    try {
      ofd.kick();
      raf.flushOne();
      expect(raf.count).toBe(0);
      (proxyOf(ofd) as unknown as { handleKeyDown(...args: unknown[]): void }).handleKeyDown(
        'KeyA',
        false,
        false,
        false,
        false,
        1,
      );
      expect(raf.count).toBe(1);
    } finally {
      ofd.destroy();
    }
  });

  it('renderFrame via the proxy is kick-exempt (no self-continuation)', () => {
    const { ofd } = makeOfd([-1]);
    try {
      ofd.kick();
      raf.flushOne();
      proxyOf(ofd).renderFrame();
      expect(raf.count).toBe(0);
    } finally {
      ofd.destroy();
    }
  });

  it('blink deadline arms a wake timer; fire renders once then suspends', () => {
    const { ofd } = makeOfd([500, -1]);
    try {
      ofd.kick();
      raf.flushOne();
      expect(vi.getTimerCount()).toBe(1);
      vi.advanceTimersByTime(500);
      expect(raf.count).toBe(1);
      raf.flushOne();
      expect(raf.count).toBe(0);
      expect(vi.getTimerCount()).toBe(0);
    } finally {
      ofd.destroy();
    }
  });

  it('pauseRendering drops pending work; resumeRendering revives', () => {
    const { ofd } = makeOfd([500]);
    try {
      ofd.kick();
      raf.flushOne();
      expect(vi.getTimerCount()).toBe(1);
      ofd.pauseRendering();
      expect(vi.getTimerCount()).toBe(0);
      expect(raf.count).toBe(0);
      ofd.resumeRendering();
      expect(raf.count).toBe(1);
    } finally {
      ofd.destroy();
    }
  });

  it('destroy is final', () => {
    const { ofd } = makeOfd([0]);
    ofd.destroy();
    ofd.kick();
    ofd.resumeRendering();
    expect(raf.count).toBe(0);
  });
});
