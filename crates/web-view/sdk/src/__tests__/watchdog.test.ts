import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { Ofd } from '../index';
import { RafStub } from './helpers';

type WasmSurface = Parameters<typeof Ofd.__testNew>[0];

function makeOfd(nexts: number[]): Ofd {
  const queue = [...nexts];
  const wasm = {
    renderFrame(): number {
      return queue.length > 0 ? (queue.shift() as number) : -1;
    },
  } as unknown as WasmSurface;
  return Ofd.__testNew(wasm, document.createElement('canvas'));
}

function setHidden(hidden: boolean): void {
  Object.defineProperty(document, 'hidden', {
    configurable: true,
    get: () => hidden,
  });
}

describe('Ofd watchdog', () => {
  let raf: RafStub;

  beforeEach(() => {
    vi.useFakeTimers();
    raf = new RafStub();
    raf.install();
  });

  afterEach(() => {
    vi.useRealTimers();
    vi.unstubAllGlobals();
    // jsdom shares document/window within a test file: restore visibility.
    setHidden(false);
  });

  it('visibilitychange hidden silences the scheduler entirely', () => {
    const ofd = makeOfd([500]);
    try {
      ofd.kick();
      raf.flushOne();
      expect(vi.getTimerCount()).toBe(1); // blink timer armed
      setHidden(true);
      document.dispatchEvent(new Event('visibilitychange'));
      expect(vi.getTimerCount()).toBe(0);
      expect(raf.count).toBe(0);
      ofd.kick(); // gated while hidden
      expect(raf.count).toBe(0);
    } finally {
      ofd.destroy();
    }
  });

  it('visibilitychange visible kicks one frame', () => {
    const ofd = makeOfd([-1]);
    try {
      setHidden(true);
      document.dispatchEvent(new Event('visibilitychange'));
      setHidden(false);
      document.dispatchEvent(new Event('visibilitychange'));
      expect(raf.count).toBe(1);
    } finally {
      ofd.destroy();
    }
  });

  it('window focus kicks', () => {
    const ofd = makeOfd([-1]);
    try {
      ofd.kick();
      raf.flushOne(); // -> suspended
      window.dispatchEvent(new Event('focus'));
      expect(raf.count).toBe(1);
    } finally {
      ofd.destroy();
    }
  });
});
