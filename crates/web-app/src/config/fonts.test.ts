import { afterEach, describe, expect, it } from 'vitest';
import { setFileHost, type FileHost } from '../host';
import { manifestUrl } from 'virtual:rword-fonts';
import {
  FONT_SLICES,
  LEGACY_FONTS,
  SYSTEM_FONT_FILES,
  resolveFontConfig,
} from './fonts';

// 能力探测驱动通道分流（draft 2026-10-17 §1.2）：注入桥有无 readSystemFont
// 决定浏览器/桌面路径，浏览器分支必须与既有配置逐字节等价（e2e 零改动是
// 验收线）。

const browserHost: FileHost = { open: async () => null, save: async () => false };

function desktopHost(map: Record<string, Uint8Array | null>): FileHost {
  return {
    ...browserHost,
    async readSystemFont(fileName) {
      if (fileName in map) {
        return map[fileName];
      }
      throw new Error(`unexpected file: ${fileName}`);
    },
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

  it('桌面：白名单逐个读取，读到的进整字体通道，manifest 只留 Noto（一族一通道）', async () => {
    const simsun = new Uint8Array([1, 2, 3]);
    const simhei = new Uint8Array([4]);
    setFileHost(
      desktopHost({
        'simsun.ttc': simsun,
        'simhei.ttf': simhei,
        'msyh.ttc': null,
        'arial.ttf': null,
        'times.ttf': null,
        'calibri.ttf': null,
      }),
    );
    const cfg = await resolveFontConfig();
    expect(cfg.fonts).toEqual([{ data: simsun }, { data: simhei }]);
    expect(cfg.fontSlices?.manifests).toEqual([manifestUrl]);
    expect(cfg.fontSlices?.includeDefaultNoto).toBe(false);
    expect(cfg.fontSlices?.defaultFamily).toBe('notosanscjksc');
  });

  it('桌面：全部读取失败 → 空整字体列表 + Noto 切片照常（纯降级）', async () => {
    setFileHost(desktopHost(Object.fromEntries(SYSTEM_FONT_FILES.map((f) => [f, null]))));
    const cfg = await resolveFontConfig();
    expect(cfg.fonts).toEqual([]);
    expect(cfg.fontSlices?.manifests).toEqual([manifestUrl]);
  });

  it('桌面：抛异常的文件与缺失同途跳过，不中断后续文件', async () => {
    setFileHost(
      desktopHost({
        'simsun.ttc': new Uint8Array([9]),
        // 其余文件不在 map → readSystemFont 抛异常 → catch 跳过
      }),
    );
    const cfg = await resolveFontConfig();
    expect(cfg.fonts).toEqual([{ data: new Uint8Array([9]) }]);
  });

  it('白名单六文件与 tauri 壳 ALLOWED_FONT_FILES 一一对应', () => {
    expect([...SYSTEM_FONT_FILES]).toEqual([
      'simsun.ttc',
      'simhei.ttf',
      'msyh.ttc',
      'arial.ttf',
      'times.ttf',
      'calibri.ttf',
    ]);
  });
});
