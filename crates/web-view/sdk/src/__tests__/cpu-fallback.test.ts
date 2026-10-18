import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { Ofd, type OfdConfig } from '../index';

// vi.mock 的工厂被提升到 import 之前——句柄经 vi.hoisted 取回，逐测试
// 编排 resolve/reject。mock 路径相对测试文件解析（../../dist），与
// index.ts 里 ../dist/rofd_web_view.js 指向同一模块，动态 import 同样
// 被拦截。
const wasmMocks = vi.hoisted(() => ({
  init: vi.fn(async () => {}),
  createWasmOfd: vi.fn(),
  createWasmOfdCpu: vi.fn(),
}));

vi.mock('../../dist/rofd_web_view.js', () => ({
  default: wasmMocks.init,
  create_wasm_ofd: wasmMocks.createWasmOfd,
  create_wasm_ofd_cpu: wasmMocks.createWasmOfdCpu,
}));

/** 合成 WasmOfd 表面：init 路径无条件触碰的最小集合（canvas.focus() 会
 * 同步触发 focus 监听 → handleFocusGained；onWarning 配置则注册 slot）。 */
function makeSurface(cpu: boolean) {
  return {
    renderFrame: vi.fn((): number => -1),
    handleResize: vi.fn(),
    handleFocusGained: vi.fn(),
    handleFocusLost: vi.fn(),
    setOnPointerCursor: vi.fn(),
    setOnCopy: vi.fn(),
    setOnWarning: vi.fn(),
    isCpuFallback: vi.fn((): boolean => cpu),
  };
}

/** jsdom 的 navigator 默认没有 gpu：按需定义/清除，测完还原。 */
function setNavigatorGpu(value: object | undefined): void {
  if (value === undefined) {
    delete (navigator as { gpu?: unknown }).gpu;
  } else {
    Object.defineProperty(navigator, 'gpu', { value, configurable: true });
  }
}

/** 排空微任务（deferCb 的 onWarning 在 queueMicrotask 里到达，FIFO
 * 保证本 resolve 排在其后——await 它即已消费之前的微任务）。 */
function flushMicrotasks(): Promise<void> {
  return new Promise((resolve) => queueMicrotask(resolve));
}

describe('Ofd.init cpuFallback 分流', () => {
  let container: HTMLElement;
  let info: ReturnType<typeof vi.spyOn>;

  beforeEach(() => {
    container = document.createElement('div');
    document.body.appendChild(container);
    info = vi.spyOn(console, 'info').mockImplementation(() => {});
    wasmMocks.init.mockClear();
    wasmMocks.createWasmOfd.mockReset();
    wasmMocks.createWasmOfdCpu.mockReset();
  });

  afterEach(() => {
    setNavigatorGpu(undefined);
    container.remove();
    vi.restoreAllMocks();
  });

  it('WebGPU 工厂拒绝 + 默认配置 → CPU 工厂接管，init 成功且 isCpuFallback=true', async () => {
    setNavigatorGpu({});
    wasmMocks.createWasmOfd.mockRejectedValueOnce(new Error('adapter request failed'));
    wasmMocks.createWasmOfdCpu.mockResolvedValueOnce(makeSurface(true));

    const onWarning = vi.fn();
    const ofd = await Ofd.init(container, { onWarning } satisfies OfdConfig);
    try {
      expect(wasmMocks.createWasmOfd).toHaveBeenCalledTimes(1);
      expect(wasmMocks.createWasmOfdCpu).toHaveBeenCalledTimes(1);
      // 默认调优参数（未传 cpuFallbackTuning）与同一 canvas 实例。
      expect(wasmMocks.createWasmOfdCpu.mock.calls[0][0]).toBeInstanceOf(HTMLCanvasElement);
      expect(wasmMocks.createWasmOfdCpu.mock.calls[0].slice(1)).toEqual([5, 8_000_000]);
      expect(ofd.isCpuFallback).toBe(true);
      // 一次性降级提示（Task 10 冒烟 grep 的原文）。
      expect(info).toHaveBeenCalledTimes(1);
      expect(info).toHaveBeenCalledWith(
        '[rofd] WebGPU 不可用，已切换 CPU 软渲染（页位图缓存已启用）',
      );
      // 宿主经 onWarning 收到降级告知（deferCb 微任务时机）。
      await flushMicrotasks();
      expect(onWarning).toHaveBeenCalledWith([
        'cpu-fallback: WebGPU unavailable, rendering on CPU',
      ]);
    } finally {
      ofd.destroy();
    }
  });

  it('cpuFallback: false + 拒绝 → init 直接拒绝，文案提及硬件加速，不触 CPU 工厂', async () => {
    setNavigatorGpu({});
    wasmMocks.createWasmOfd.mockRejectedValueOnce(new Error('no suitable adapter'));

    await expect(
      Ofd.init(container, { cpuFallback: false } satisfies OfdConfig),
    ).rejects.toThrow('硬件加速');
    expect(wasmMocks.createWasmOfdCpu).not.toHaveBeenCalled();
    expect(info).not.toHaveBeenCalled();
  });

  it('navigator.gpu 不存在 → 同样落入 CPU 工厂（WebGPU 工厂不被调用）', async () => {
    setNavigatorGpu(undefined);
    wasmMocks.createWasmOfdCpu.mockResolvedValueOnce(makeSurface(true));

    const ofd = await Ofd.init(container);
    try {
      expect(wasmMocks.createWasmOfd).not.toHaveBeenCalled();
      expect(wasmMocks.createWasmOfdCpu).toHaveBeenCalledTimes(1);
      expect(ofd.isCpuFallback).toBe(true);
      expect(info).toHaveBeenCalledTimes(1);
    } finally {
      ofd.destroy();
    }
  });

  it('cpuFallbackTuning 透传到 CPU 工厂', async () => {
    setNavigatorGpu(undefined);
    wasmMocks.createWasmOfdCpu.mockResolvedValueOnce(makeSurface(true));

    const ofd = await Ofd.init(container, {
      cpuFallbackTuning: { maxCachedPages: 9, maxPagePixels: 1_000_000 },
    } satisfies OfdConfig);
    try {
      expect(wasmMocks.createWasmOfdCpu.mock.calls[0].slice(1)).toEqual([9, 1_000_000]);
    } finally {
      ofd.destroy();
    }
  });
});
