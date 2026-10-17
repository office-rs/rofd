import { describe, expect, it, vi } from 'vitest';
import type { FontRuntime, FontSlice } from '@office-rs/font-runtime';
import {
  ensureFonts, scheduleFontWatch, filterLiveManifests, WATCH_DEBOUNCE_MS,
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
    // notosans 在前、simsun 在后：defaultFirst 若不把 default 族前置则无法通过（防同义反复）。
    selectInitial: vi.fn(() => ({ families: [
      { family: 'notosans', load: 'whole', sliceIds: [] },
      { family: 'simsun', load: 'slices', sliceIds: ['s1'] },
    ] })),
    watchText: vi.fn(() => ({
      families: opts.watch?.map((s) => ({ family: s.family, load: 'slices', sliceIds: [s.id] })) ?? [],
    })),
    ensure: async function* () {
      const list = [
        ...(opts.slices ?? [
          { family: 'notosans', id: '__whole__' },
          { family: 'simsun', id: 's1' },
        ]),
        ...(opts.watch ?? []),
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
    expect(order).toEqual(['simsun', 'notosans']);
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
    // delta = 两次 collect 做差（上次为空集）→ watchText 收到整个新增码位集。
    expect(rt.watchText).toHaveBeenCalledWith([0x4e2d]);
    // watch 计划的切片经 ensure 流到 registerFontSlice。
    expect(wasm.registered).toContainEqual({ family: 'simsun', id: 's9' });
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
    // 首拍对空基线本就是 delta（watchText 一次）；需求签名未变 → 第二拍在
    // key 检查处短路，不再产生 delta 计划。
    expect(rt.watchText).toHaveBeenCalledTimes(1);
    watch.cancel();
    vi.useRealTimers();
  });
});

describe('filterLiveManifests', () => {
  it('keeps live manifests in order, dropping 404 and network failures', async () => {
    const fetchMock = vi.fn(async (url: string) => {
      if (url.endsWith('/live.json')) {
        return { ok: true, headers: new Headers({ 'content-type': 'application/json' }) } as Response;
      }
      if (url.endsWith('/gone.json')) { return { ok: false, headers: new Headers() } as Response; }
      throw new TypeError('network down');
    });
    const info = vi.spyOn(console, 'info').mockImplementation(() => {});
    vi.stubGlobal('fetch', fetchMock);
    try {
      const live = await filterLiveManifests([
        'https://fonts.example/gone.json',
        'https://fonts.example/live.json',
        'https://fonts.example/down.json',
      ]);
      expect(live).toEqual(['https://fonts.example/live.json']);
      expect(fetchMock).toHaveBeenCalledTimes(3);
    } finally {
      vi.unstubAllGlobals();
      info.mockRestore();
    }
  });

  it('drops ok responses whose content-type is not JSON (dev Vite SPA fallback regression)', async () => {
    // dev 态 Vite html-fallback 把缺失 manifest 回退成 index.html：
    // 200 + text/html——res.ok 为真也必须拒收（spec §5 降级不告警）。
    const fetchMock = vi.fn(async (url: string) => {
      if (url.endsWith('/real.json')) {
        return { ok: true, headers: new Headers({ 'content-type': 'application/json' }) } as Response;
      }
      // +json 后缀（RFC 6839 结构化语法类型）同样放行。
      if (url.endsWith('/vendor.json')) {
        return { ok: true, headers: new Headers({ 'content-type': 'application/vnd.rofd+json' }) } as Response;
      }
      return { ok: true, headers: new Headers({ 'content-type': 'text/html; charset=utf-8' }) } as Response;
    });
    const info = vi.spyOn(console, 'info').mockImplementation(() => {});
    vi.stubGlobal('fetch', fetchMock);
    try {
      const live = await filterLiveManifests([
        'https://fonts.example/spa-fallback.json',
        'https://fonts.example/real.json',
        'https://fonts.example/vendor.json',
      ]);
      expect(live).toEqual(['https://fonts.example/real.json', 'https://fonts.example/vendor.json']);
    } finally {
      vi.unstubAllGlobals();
      info.mockRestore();
    }
  });
});
