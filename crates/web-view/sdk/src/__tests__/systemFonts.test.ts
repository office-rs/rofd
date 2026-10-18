import { describe, expect, it } from 'vitest';
import { FontRuntime } from '@office-rs/font-runtime';
import {
  createSystemFlow,
  ensureFonts,
  manifestFamilyKeys,
  satisfySystemFamilies,
  mergeFontFamilyLists,
  type SystemFontEntry,
  type SystemFontProvider,
  type WasmFontSurface,
} from '../fontFlow';
import { Ofd } from '../index';

// 系统字体流（draft 2026-10-18 §4）：假 provider + 真 fontFlow。键宇宙用
// 内核归一表的迷你替身（宋体/SimSun→simsun）——真表在 Rust 侧，这里只测
// "经 normalizeFontName 归一后两侧可比"的机制。

function fakeSurface(over: Partial<WasmFontSurface> = {}): WasmFontSurface & {
  registered: Uint8Array[];
} {
  const registered: Uint8Array[] = [];
  return {
    registered,
    collectFontRequirements: () => ({ families: [], codepoints: [] }),
    registerFontSlice: () => true,
    registerFont: (b) => {
      registered.push(b);
      return true;
    },
    normalizeFontName: (n) => (n === '宋体' || n === 'SimSun' ? 'simsun' : n.toLowerCase()),
    ...over,
  };
}

function fakeProvider(
  entries: SystemFontEntry[],
  readImpl?: (entry: SystemFontEntry) => Promise<Uint8Array | null>,
): SystemFontProvider & { reads: SystemFontEntry[] } {
  const reads: SystemFontEntry[] = [];
  return {
    reads,
    list: async () => entries,
    read: async (entry) => {
      reads.push(entry);
      return readImpl ? readImpl(entry) : new Uint8Array([1]);
    },
  };
}

const SIMSUN_ZH: SystemFontEntry = { family: '宋体', fileName: 'simsun.ttc' };
const SIMSUN_EN: SystemFontEntry = { family: 'SimSun', fileName: 'simsun.ttc' };
const KAITI: SystemFontEntry = { family: '楷体', fileName: 'simkai.ttf' };

describe('createSystemFlow', () => {
  it('list 归一建 map（同键首见胜出）且 familiesRaw 保留原始双侧名', async () => {
    const p = fakeProvider([SIMSUN_ZH, SIMSUN_EN, KAITI]);
    const flow = await createSystemFlow(fakeSurface(), p, '宋体');
    expect(flow.fileByFamily.get('simsun')).toBe('simsun.ttc');
    expect(flow.fileByFamily.get('楷体')).toBe('simkai.ttf');
    expect(flow.familiesRaw).toEqual(['宋体', 'SimSun', '楷体']);
    expect(flow.defaultFamilyKey).toBe('simsun');
  });

  it('list 失败 → 空流（桌面纯降级）', async () => {
    const p: SystemFontProvider = {
      list: async () => {
        throw new Error('ipc gone');
      },
      read: async () => null,
    };
    const flow = await createSystemFlow(fakeSurface(), p);
    expect(flow.fileByFamily.size).toBe(0);
    expect(flow.familiesRaw).toEqual([]);
  });
});

describe('satisfySystemFamilies', () => {
  it('声明族读本机注册一次；settled 使重复遍零读（draft §6.4）', async () => {
    const p = fakeProvider([SIMSUN_ZH]);
    const surface = fakeSurface({
      collectFontRequirements: () => ({ families: ['simsun'], codepoints: [] }),
    });
    const flow = await createSystemFlow(surface, p);
    await satisfySystemFamilies(surface, flow, ['simsun'], new Set());
    await satisfySystemFamilies(surface, flow, ['simsun'], new Set());
    expect(p.reads).toHaveLength(1);
    expect(surface.registered).toHaveLength(1);
  });

  it('清单覆盖族永不咨询 provider（一族一通道）', async () => {
    const p = fakeProvider([SIMSUN_ZH]);
    const surface = fakeSurface();
    const flow = await createSystemFlow(surface, p);
    await satisfySystemFamilies(surface, flow, ['simsun'], new Set(['simsun']));
    expect(p.reads).toHaveLength(0);
  });

  it('本机没有的族：map miss → settled，不读不报错', async () => {
    const p = fakeProvider([]);
    const surface = fakeSurface();
    const flow = await createSystemFlow(surface, p);
    await satisfySystemFamilies(surface, flow, ['simsun'], new Set());
    expect(p.reads).toHaveLength(0);
    expect([...flow.settled]).toEqual(['simsun']);
  });

  it('read 返回 null / 抛错都 settle 并经 onError 上报，不向调用方抛', async () => {
    const errors: string[] = [];
    const onError = (e: { family: string; message: string }): void => {
      errors.push(`${e.family}:${e.message}`);
    };
    const p = fakeProvider(
      [SIMSUN_ZH, KAITI],
      async (entry) => {
        if (entry.family === '宋体') {
          return null; // 不可用 → 替代链兜底
        }
        throw new Error('ipc boom');
      },
    );
    const surface = fakeSurface();
    const flow = await createSystemFlow(surface, p);
    await satisfySystemFamilies(surface, flow, ['simsun', '楷体'], new Set(), onError);
    expect(surface.registered).toHaveLength(0);
    expect(flow.settled.has('simsun')).toBe(true);
    expect(flow.settled.has('楷体')).toBe(true);
    expect(errors.some((m) => m.includes('楷体'))).toBe(true);
  });

  it('defaultFamily 恒入遍（桌面 D5：宋体锚 default）', async () => {
    const p = fakeProvider([SIMSUN_ZH]);
    const surface = fakeSurface();
    const flow = await createSystemFlow(surface, p, '宋体');
    await satisfySystemFamilies(surface, flow, [], new Set());
    expect(p.reads).toHaveLength(1);
    expect(p.reads[0].fileName).toBe('simsun.ttc');
  });

  it('surface 无 registerFont（异常配置）→ 静默不作用', async () => {
    const p = fakeProvider([SIMSUN_ZH]);
    const surface = fakeSurface({ registerFont: undefined });
    const flow = await createSystemFlow(surface, p);
    await satisfySystemFamilies(surface, flow, ['simsun'], new Set());
    expect(p.reads).toHaveLength(0);
  });
});

describe('manifestFamilyKeys / mergeFontFamilyLists', () => {
  it('键与显示名双侧归一入集合（draft §6.2 坑的 rofd 形态）', () => {
    const surface = fakeSurface();
    const keys = manifestFamilyKeys(surface, {
      families: {
        notosanscjksc: { displayName: 'Noto Sans CJK SC' },
      },
    });
    expect(keys.has('notosanscjksc')).toBe(true);
    expect(keys.has('noto sans cjk sc')).toBe(true);
  });

  it('picker 合并：已列族原序，系统族去重排序追加', () => {
    const merged = mergeFontFamilyLists(['Noto Sans CJK SC', '宋体'], [
      'SimSun',
      '宋体',
      'Arial',
    ]);
    expect(merged[0]).toBe('Noto Sans CJK SC');
    expect(merged.slice(1)).toEqual(['宋体', 'Arial', 'SimSun'].sort((a, b) =>
      a.localeCompare(b),
    ));
    expect(new Set(merged).size).toBe(merged.length);
  });
});

describe('desktop pure-channel integration（真 FontRuntime 空清单）', () => {
  it('空 manifests + systemFonts：ensureFonts 后系统族已注册', async () => {
    const p = fakeProvider([SIMSUN_ZH, KAITI]);
    const surface = fakeSurface({
      collectFontRequirements: () => ({ families: ['楷体'], codepoints: [] }),
    });
    const runtime = new FontRuntime({ manifests: [], includeDefaultNoto: false });
    const flow = await createSystemFlow(surface, p, '宋体');
    const ok = await ensureFonts(surface, runtime, { system: flow });
    expect(ok).toBe(true);
    // default 宋体 + 声明楷体都读了；每个文件只读一次（两族不同文件）。
    expect(p.reads.map((e) => e.fileName).sort()).toEqual(['simkai.ttf', 'simsun.ttc']);
    expect(surface.registered).toHaveLength(2);
  });

  it('getFontFamilies：manifest displayName ∪ 系统原始名（经 __testNew 注入态）', async () => {
    const ofd = Ofd.__testNew(
      fakeSurface() as unknown as Parameters<typeof Ofd.__testNew>[0],
      document.createElement('canvas'),
    );
    const inner = ofd as unknown as {
      fontRuntime?: unknown;
      systemFlow?: { familiesRaw: string[] };
    };
    inner.fontRuntime = {
      ready: async () => ({
        families: { notosanscjksc: { displayName: 'Noto Sans CJK SC' } },
      }),
    };
    inner.systemFlow = { familiesRaw: ['宋体', 'SimSun'] };
    const fams = await ofd.getFontFamilies();
    expect(fams[0]).toBe('Noto Sans CJK SC');
    expect(fams).toContain('宋体');
    expect(fams).toContain('SimSun');
    expect(new Set(fams).size).toBe(fams.length);
    ofd.destroy();
  });
});
