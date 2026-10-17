import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { Ofd } from '../index';
import { RafStub } from './helpers';

// 切片流接线测试（fix round 1）：mock fontFlow 的全部编排函数与
// FontRuntime 构造器，用 __testNew 造真 Ofd 实例（proxy + scheduler 全真，
// rAF 用 RafStub 吸收），驱动 startFontFlow / loadOfd / destroy，断言
// init→fontFlow 接线与状态机（off→init→ready|off）。
const {
  filterLiveManifests,
  ensureFonts,
  scheduleFontWatch,
  FontRuntimeCtor,
} = vi.hoisted(() => ({
  filterLiveManifests: vi.fn(),
  ensureFonts: vi.fn(),
  scheduleFontWatch: vi.fn(() => ({ kick: vi.fn(), cancel: vi.fn() })),
  FontRuntimeCtor: vi.fn(),
}));

vi.mock('../fontFlow', () => ({
  filterLiveManifests,
  ensureFonts,
  scheduleFontWatch,
}));
vi.mock('@office-rs/font-runtime', () => ({ FontRuntime: FontRuntimeCtor }));

type WasmSurface = Parameters<typeof Ofd.__testNew>[0];
type Watch = { kick: () => void; cancel: () => void };

const MANIFESTS = ['https://fonts.example/slices.json'];

/** 冲两拍宏任务：retryFontFlow 的 await 链（mock 同步 resolve）与 deferCb
 * 排的微任务在第一个 setTimeout 回调前已全部排空。 */
async function settle(): Promise<void> {
  await new Promise<void>((r) => { setTimeout(r, 0); });
  await new Promise<void>((r) => { setTimeout(r, 0); });
}

function lastWatch(): Watch {
  const w = scheduleFontWatch.mock.results[scheduleFontWatch.mock.results.length - 1]?.value;
  if (!w) {
    throw new Error('no watch scheduled');
  }
  return w as Watch;
}

describe('Ofd slice font-flow wiring', () => {
  let raf: RafStub;
  const instances: Ofd[] = [];

  beforeEach(() => {
    vi.clearAllMocks();
    raf = new RafStub();
    raf.install();
    filterLiveManifests.mockResolvedValue(MANIFESTS);
  });

  afterEach(() => {
    for (const ofd of instances.splice(0)) {
      ofd.destroy();
    }
    vi.unstubAllGlobals();
  });

  function makeOfd(): Ofd {
    const wasm = {
      collectFontRequirements: vi.fn(() => ({ families: [], codepoints: [] })),
      registerFontSlice: vi.fn(() => true),
      loadOfd: vi.fn(),
      renderFrame: vi.fn(() => -1),
    } as unknown as WasmSurface;
    const ofd = Ofd.__testNew(wasm, document.createElement('canvas'));
    instances.push(ofd);
    return ofd;
  }

  it('startFontFlow: live manifests into FontRuntime; includeDefaultNoto defaults to false', async () => {
    const ofd = makeOfd();
    filterLiveManifests.mockResolvedValue(['https://fonts.example/live.json']);
    ofd.startFontFlow({ manifests: MANIFESTS });
    await settle();
    expect(filterLiveManifests).toHaveBeenCalledWith(MANIFESTS);
    expect(FontRuntimeCtor).toHaveBeenCalledTimes(1);
    expect(FontRuntimeCtor).toHaveBeenCalledWith({
      manifests: ['https://fonts.example/live.json'],
      includeDefaultNoto: false,
    });
  });

  it('forwards includeDefaultNoto: true when configured', async () => {
    const ofd = makeOfd();
    ofd.startFontFlow({ manifests: MANIFESTS, includeDefaultNoto: true });
    await settle();
    expect(FontRuntimeCtor).toHaveBeenCalledWith({
      manifests: MANIFESTS,
      includeDefaultNoto: true,
    });
  });

  it('empty live manifests construct no runtime (degraded); next loadOfd retries', async () => {
    const ofd = makeOfd();
    filterLiveManifests.mockResolvedValue([]);
    ofd.startFontFlow({ manifests: MANIFESTS });
    await settle();
    expect(FontRuntimeCtor).not.toHaveBeenCalled();
    expect(scheduleFontWatch).not.toHaveBeenCalled();
    ofd.loadOfd(new Uint8Array([1]));
    await settle();
    expect(filterLiveManifests).toHaveBeenCalledTimes(2);
  });

  it('ensureFonts success mounts the watcher; loadOfd while ready kicks it', async () => {
    const ofd = makeOfd();
    ensureFonts.mockResolvedValue(true);
    ofd.startFontFlow({ manifests: MANIFESTS });
    await settle();
    expect(ensureFonts).toHaveBeenCalledTimes(1);
    expect(scheduleFontWatch).toHaveBeenCalledTimes(1);
    ofd.loadOfd(new Uint8Array([1]));
    expect(lastWatch().kick).toHaveBeenCalledTimes(1);
  });

  it('ensureFonts failure mounts no watcher; next loadOfd retries the flow', async () => {
    const ofd = makeOfd();
    ensureFonts.mockResolvedValue(false);
    ofd.startFontFlow({ manifests: MANIFESTS });
    await settle();
    expect(scheduleFontWatch).not.toHaveBeenCalled();
    ofd.loadOfd(new Uint8Array([1]));
    await settle();
    expect(filterLiveManifests).toHaveBeenCalledTimes(2);
  });

  it('starting again while ready does not re-run the flow (no double watcher)', async () => {
    const ofd = makeOfd();
    ensureFonts.mockResolvedValue(true);
    ofd.startFontFlow({ manifests: MANIFESTS });
    await settle();
    // 已 ready：重入必须早退——否则双挂 watcher，泄漏一个 300ms debounce 定时器。
    ofd.startFontFlow({ manifests: MANIFESTS });
    await settle();
    expect(filterLiveManifests).toHaveBeenCalledTimes(1);
    expect(scheduleFontWatch).toHaveBeenCalledTimes(1);
  });

  it('reports font-flow errors through a deferred host callback (cannot wedge the flow)', async () => {
    const ofd = makeOfd();
    const warnings: string[][] = [];
    let seenAtSyncPoint = -1;
    ensureFonts.mockImplementation(
      async (
        _wasm: unknown,
        _runtime: unknown,
        opts: { onError?: (e: { family: string; message: string }) => void },
      ) => {
        opts.onError?.({ family: 'simsun', message: 'boom' });
        // 同步观察点：deferCb 下宿主回调尚未触达（微任务里才跑）。
        seenAtSyncPoint = warnings.length;
        return false;
      },
    );
    ofd.startFontFlow({ manifests: MANIFESTS }, (w) => { warnings.push(w); });
    await settle();
    expect(seenAtSyncPoint).toBe(0);
    expect(warnings).toEqual([['fonts: simsun boom']]);
    // 失败回 off（宿主抛错也撬不动状态机）：loadOfd 重试整个流。
    ofd.loadOfd(new Uint8Array([1]));
    await settle();
    expect(filterLiveManifests).toHaveBeenCalledTimes(2);
  });

  it('destroy cancels the mounted watcher', async () => {
    const ofd = makeOfd();
    ensureFonts.mockResolvedValue(true);
    ofd.startFontFlow({ manifests: MANIFESTS });
    await settle();
    const watch = lastWatch();
    ofd.destroy();
    expect(watch.cancel).toHaveBeenCalledTimes(1);
  });
});
