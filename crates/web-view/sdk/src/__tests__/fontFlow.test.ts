import { describe, expect, it, vi } from 'vitest';
import type { FontRuntime, FontSlice } from '@office-rs/font-runtime';
import {
  ensureFonts, scheduleFontWatch, WATCH_DEBOUNCE_MS,
  type WasmFontSurface, type FontFlowError,
} from '../fontFlow';

function fakeWasm(families: string[], codepoints: number[]): WasmFontSurface & {
  registered: Array<{ family: string; id: string }>;
} {
  const registered: Array<{ family: string; id: string }> = [];
  return {
    registered,
    collectFontRequirements: () => ({ families, codepoints }),
    registerFontSlice: (_b, family, id) => { registered.push({ family, id }); return true; },
  };
}

function fakeRuntime(opts: {
  manifest?: Record<string, unknown>;
  slices?: FontSlice[];
  watch?: FontSlice[];
} = {}) {
  const sliceFor = (s: { family: string; id: string }): FontSlice =>
    ({ family: s.family, id: s.id, bytes: new Uint8Array([1]) });
  return {
    ready: async () => opts.manifest ?? {
      families: {
        notosanscjksc: { displayName: 'Noto Sans CJK SC', slices: [] },
        simsun: { displayName: 'SimSun', slices: [] },
      },
    },
    selectInitial: vi.fn(() => ({ families: [
      { family: 'simsun', load: 'slices', sliceIds: ['s1'] },
      { family: 'notosans', load: 'whole', sliceIds: [] },
    ] })),
    watchText: vi.fn(() => ({ families: [] })),
    ensure: async function* () {
      const list = opts.slices ?? [
        { family: 'notosans', id: '__whole__' },
        { family: 'simsun', id: 's1' },
      ];
      for (const s of list) { yield sliceFor(s); }
    },
  } as unknown as FontRuntime;
}

describe('ensureFonts', () => {
  it('translates family keys to displayNames for selectInitial', async () => {
    const rt = fakeRuntime();
    const wasm = fakeWasm(['simsun'], [0x4e2d]);
    const ok = await ensureFonts(wasm, rt, {});
    expect(ok).toBe(true);
    expect(rt.selectInitial).toHaveBeenCalledWith([0x4e2d], ['SimSun']);
  });

  it('orders the default family plan first', async () => {
    // defaultFamily 条目排到 plan 首位（内核 default 锚定，spec §3.1）。
    const rt = fakeRuntime();
    const order: string[] = [];
    (rt as unknown as { ensure: () => AsyncGenerator<FontSlice, void, unknown> }).ensure =
      async function* (plan: { families: Array<{ family: string }> }) {
        for (const f of plan.families) { order.push(f.family); }
      };
    const wasm = fakeWasm([], []);
    await ensureFonts(wasm, rt, { defaultFamily: 'simsun' });
    expect(order[0]).toBe('simsun');
  });

  it('reports register failures via onError without aborting', async () => {
    const wasm = fakeWasm([], []);
    wasm.registerFontSlice = () => false;
    const errors: FontFlowError[] = [];
    const ok = await ensureFonts(wasm, fakeRuntime(), { onError: (e) => errors.push(e) });
    expect(ok).toBe(true);
    expect(errors.every((e) => e.message.includes('register'))).toBe(true);
  });

  it('manifest failure reports and returns false (no throw)', async () => {
    const rt = fakeRuntime();
    (rt as unknown as { ready: () => Promise<never> }).ready = async () => {
      throw new Error('manifest fetch failed: x');
    };
    const errors: FontFlowError[] = [];
    const ok = await ensureFonts(fakeWasm([], []), rt, { onError: (e) => errors.push(e) });
    expect(ok).toBe(false);
    expect(errors[0].family).toBe('');
  });
});

describe('scheduleFontWatch', () => {
  it('debounces kicks into one tick and plans the delta', async () => {
    vi.useFakeTimers();
    const codepoints = [0x4e2d];
    const wasm = fakeWasm(['simsun'], codepoints);
    const rt = fakeRuntime({ watch: [{ family: 'simsun', id: 's9' }] });
    const watch = scheduleFontWatch(wasm, rt, {});
    watch.kick();
    watch.kick();
    await vi.advanceTimersByTimeAsync(WATCH_DEBOUNCE_MS);
    expect(rt.selectInitial).toHaveBeenCalledTimes(1);
    watch.cancel();
    vi.useRealTimers();
  });

  it('skips the tick when requirements are unchanged', async () => {
    vi.useFakeTimers();
    const wasm = fakeWasm(['simsun'], [0x4e2d]);
    const rt = fakeRuntime();
    const watch = scheduleFontWatch(wasm, rt, {});
    watch.kick();
    await vi.advanceTimersByTimeAsync(WATCH_DEBOUNCE_MS);
    watch.kick();
    await vi.advanceTimersByTimeAsync(WATCH_DEBOUNCE_MS);
    expect(rt.selectInitial).toHaveBeenCalledTimes(1);
    watch.cancel();
    vi.useRealTimers();
  });
});
