import { afterEach, describe, expect, it, vi } from 'vitest';

// init 的字体分支是纯配置分派——测分派逻辑本身（不 boot wasm）：把 init 的
// 字体决策抽成可测静态函数 Ofd.dispatchFonts（task-9 brief Step 3），vitest
// 直接驱动。切片路径（filterLiveManifests→ensureFonts→scheduleFontWatch）
// 不在这里重测——fontFlow.test.ts 已覆盖该编排。
import { Ofd } from '../index';

/** fetch 返回 ok + arrayBuffer 的最小 Response 形状。 */
function fontResponse(bytes: number[]): Response {
  return {
    ok: true,
    arrayBuffer: async () => new Uint8Array(bytes).buffer,
  } as unknown as Response;
}

describe('Ofd.init font dispatch (dispatchFonts)', () => {
  afterEach(() => {
    vi.unstubAllGlobals();
    vi.restoreAllMocks();
  });

  it('bare init (no fonts, no fontSlices) issues zero font requests', async () => {
    const fetchMock = vi.fn(async () => {
      throw new Error('unexpected font fetch');
    });
    vi.stubGlobal('fetch', fetchMock);
    const registerFont = vi.fn(() => true);
    await Ofd.dispatchFonts({ registerFont }, undefined);
    expect(fetchMock).not.toHaveBeenCalled();
    expect(registerFont).not.toHaveBeenCalled();
  });

  it('legacy fonts: awaited loadFont + registerFont loop, url and data sources in order', async () => {
    // fetch 与 registerFont 共享一个调用日志：awaited 循环的语义是
    // fetch1→register1→fetch2→register2→register3 严格顺序（与改造前
    // 逐字节同行为）；inline data 不经 fetch 直接注册。
    const log: string[] = [];
    vi.stubGlobal('fetch', vi.fn(async (url: string) => {
      log.push(`fetch:${url}`);
      const bytes = url.endsWith('a.woff') ? [0xaa] : [0xbb];
      return fontResponse(bytes);
    }));
    const registered: number[][] = [];
    const wasmOfd = {
      registerFont(bytes: Uint8Array): boolean {
        log.push(`register:${Array.from(bytes, (b) => b.toString(16)).join(',')}`);
        registered.push(Array.from(bytes));
        return true;
      },
    };
    await Ofd.dispatchFonts(wasmOfd, [
      { url: 'https://fonts.example/a.woff' },
      { url: 'https://fonts.example/b.woff' },
      { data: new Uint8Array([0x09]) },
    ]);
    expect(log).toEqual([
      'fetch:https://fonts.example/a.woff',
      'register:aa',
      'fetch:https://fonts.example/b.woff',
      'register:bb',
      'register:9',
    ]);
    expect(registered).toEqual([[0xaa], [0xbb], [0x09]]);
  });

  it('a failed font load warns and continues to the next without rejecting', async () => {
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => {});
    vi.stubGlobal('fetch', vi.fn(async (url: string) => {
      if (url.endsWith('bad.woff')) {
        return { ok: false } as unknown as Response;
      }
      return fontResponse([7]);
    }));
    const registered: number[][] = [];
    const wasmOfd = {
      registerFont(bytes: Uint8Array): boolean {
        registered.push(Array.from(bytes));
        return true;
      },
    };
    await expect(
      Ofd.dispatchFonts(wasmOfd, [
        { url: 'https://fonts.example/bad.woff' },
        { url: 'https://fonts.example/good.woff' },
      ]),
    ).resolves.toBeUndefined();
    expect(registered).toEqual([[7]]);
    expect(warn).toHaveBeenCalledTimes(1);
  });
});
