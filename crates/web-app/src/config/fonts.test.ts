import { afterEach, describe, expect, it } from 'vitest';
import { setFileHost, type FileHost } from '../host';
import { manifestUrl } from 'virtual:rword-fonts';
import { FONT_SLICES, LEGACY_FONTS, resolveFontConfig } from './fonts';

// 能力探测驱动通道分流（draft 2026-10-18 §5）：**双能力**（list + read）才走
// 桌面纯系统通道；浏览器分支必须与切片改造前逐字节等价（e2e 零改动是
// 验收线）。

const browserHost: FileHost = { open: async () => null, save: async () => false };

/** 桌面桥：list 返回清单，read 按 fileName 应答（未知名 → null）。 */
function desktopHost(entries: { family: string; fileName: string }[]): FileHost {
  return {
    ...browserHost,
    listSystemFonts: async () => entries,
    readSystemFont: async (fileName) =>
      entries.some((e) => e.fileName === fileName) ? new Uint8Array([1]) : null,
  };
}

/** 只有 read 没有 list 的宿主：双能力探测不满足 → 浏览器分支。 */
function halfHost(): FileHost {
  return {
    ...browserHost,
    readSystemFont: async () => new Uint8Array([1]),
  };
}

afterEach(() => {
  setFileHost(browserHost);
});

describe('resolveFontConfig', () => {
  it('浏览器（无能力）：与既有配置逐字节等价', async () => {
    setFileHost(browserHost);
    const cfg = await resolveFontConfig();
    expect(cfg).toEqual(FONT_SLICES ? { fontSlices: FONT_SLICES } : { fonts: LEGACY_FONTS });
  });

  it('单能力（有 read 无 list）不算桌面：仍走浏览器分支', async () => {
    setFileHost(halfHost());
    const cfg = await resolveFontConfig();
    expect(cfg).toEqual(FONT_SLICES ? { fontSlices: FONT_SLICES } : { fonts: LEGACY_FONTS });
  });

  it('桌面（双能力）：纯系统通道——空清单 + provider + 宋体默认，无整字体数组', async () => {
    setFileHost(desktopHost([{ family: '宋体', fileName: 'simsun.ttc' }]));
    const cfg = await resolveFontConfig();
    expect(cfg.fonts).toBeUndefined();
    expect(cfg.fontSlices).toEqual({ manifests: [], includeDefaultNoto: false });
    expect(cfg.defaultFontFamily).toBe('宋体');
    expect(typeof cfg.systemFonts?.list).toBe('function');
    expect(typeof cfg.systemFonts?.read).toBe('function');
    // provider 桥接 host：read 按文件名应答，未知名 → null（该族 settle）。
    expect(await cfg.systemFonts?.read({ family: '宋体', fileName: 'simsun.ttc' })).toBeTruthy();
    expect(await cfg.systemFonts?.read({ family: 'x', fileName: 'nope.ttf' })).toBeNull();
    expect(await cfg.systemFonts?.list()).toEqual([
      { family: '宋体', fileName: 'simsun.ttc' },
    ]);
  });

  it('provider 不在本模块读任何字节（懒加载归 SDK 时机）', async () => {
    let reads = 0;
    const host: FileHost = {
      ...desktopHost([]),
      readSystemFont: async () => {
        reads += 1;
        return new Uint8Array([1]);
      },
    };
    setFileHost(host);
    await resolveFontConfig();
    expect(reads).toBe(0);
  });
});

// manifestUrl 桩值参与等价断言的传递性（避免桩漂移无人察觉）。
void manifestUrl;
