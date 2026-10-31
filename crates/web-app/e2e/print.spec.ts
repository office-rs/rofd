// 打印流 e2e（print spec 2026-10-31 §7）：exportPdf 字节 / planPages 范围
// 语法 / iframe 打印通道（挂载 + window.print 调用 + 旧帧清理）。
//
// 启动套路同 fonts.spec.ts（playwright.config 起 dev server，无额外 stub）。
// 编辑器句柄走 DEV 态测试缝 `window.__ofd`（App.vue，Ofd.init resolve 后挂）；
// 启动序列随后 fetch public/sample.ofd 并 loadOfd，所以「文档就绪」判据 =
// planPages('All') 能解出非空页列（page_plan 对 0 页文档报 "document has no
// pages"），比固定等待可靠。
//
// window.print stub 记账：printViaIframe 调的是 **iframe 的 contentWindow**
// .print()，不是主窗口。addInitScript 会在每个 frame（attach/导航时）执行，
// 故每个 frame 都装上记录器；about:blank / blob: 子帧与顶层同源，经
// window.top 把计数落到主窗口。另补一道 contentWindow getter 补丁兜底 plugin
// document 提交（那种文档不跑 init script）：每次访问 contentWindow 先给子窗
// 盖上记录器再返回原窗。本 Chromium 实测（探针验证）：blob-PDF 以常规同源
// 文档提交、init script 已覆盖；getter 补丁仅为该行为变化时的保险。
import { expect, test, type Page } from '@playwright/test';

/** SDK Ofd 实例上本 spec 消费的打印面（dev 缝 `window.__ofd`）。 */
interface OfdPrintApi {
  exportPdf(options?: {
    watermark?: boolean;
    annotations?: boolean;
    seal?: boolean;
    pages?: number[];
  }): Promise<Uint8Array>;
  planPages(
    range: { kind: 'All' | 'Current' | 'Custom'; spec?: string },
    parity: 'All' | 'Odd' | 'Even',
    currentPage: number,
  ): Promise<number[]>;
  print(options?: {
    watermark?: boolean;
    annotations?: boolean;
    seal?: boolean;
    pages?: number[];
  }): Promise<void>;
}

/** 等编辑器就绪：`__ofd` 挂上且文档已载入（All 页列非空）。 */
async function waitForDocument(page: Page): Promise<void> {
  await expect
    .poll(
      async () =>
        page.evaluate(async () => {
          const ofd = (window as unknown as { __ofd?: OfdPrintApi }).__ofd;
          if (!ofd) {
            return false;
          }
          try {
            return (await ofd.planPages({ kind: 'All' }, 'All', 1)).length > 0;
          } catch {
            return false;
          }
        }),
      { timeout: 20_000 },
    )
    .toBe(true);
}

test('exportPdf returns valid PDF bytes', async ({ page }) => {
  await page.goto('/');
  await waitForDocument(page);
  const info = await page.evaluate(async () => {
    const ofd = (window as unknown as { __ofd?: OfdPrintApi }).__ofd;
    if (!ofd) {
      throw new Error('__ofd handle missing');
    }
    const bytes = await ofd.exportPdf();
    // 在页面侧只取前缀 + 长度，避免把整份 PDF 序列化回 Node。
    const head = String.fromCharCode(...bytes.subarray(0, 5));
    return { head, length: bytes.length };
  });
  expect(info.head).toBe('%PDF-');
  expect(info.length).toBeGreaterThan(1_000);
});

test('planPages resolves range grammar', async ({ page }) => {
  await page.goto('/');
  await waitForDocument(page);
  const res = await page.evaluate(async () => {
    const ofd = (window as unknown as { __ofd?: OfdPrintApi }).__ofd;
    if (!ofd) {
      throw new Error('__ofd handle missing');
    }
    return {
      all: await ofd.planPages({ kind: 'All' }, 'All', 1),
      // 覆盖语法上限 5000 的 Custom 范围：越界页被静默剔除（Word 行为），
      // 恰好剩文档实际页 —— 与 All 两条独立代码路径互证页数，不硬编码。
      capped: await ofd.planPages({ kind: 'Custom', spec: '1-5000' }, 'All', 1),
      custom1: await ofd.planPages({ kind: 'Custom', spec: '1' }, 'All', 1),
    };
  });
  expect(res.all.length).toBeGreaterThan(0);
  expect(res.all).toEqual(res.capped);
  expect(res.custom1).toEqual([1]);
});

test('print mounts iframe and calls print', async ({ page }) => {
  await page.addInitScript(() => {
    const record = (): void => {
      const top = window.top as unknown as { __printCalls?: number } | null;
      if (top && typeof top.__printCalls === 'number') {
        top.__printCalls++;
      }
    };
    if (window.top === window) {
      (window as unknown as { __printCalls: number }).__printCalls = 0;
    }
    window.print = record;
    // 兜底：若 blob-PDF 以 plugin document 提交（不跑 init script），在
    // contentWindow 访问边界盖章——print.ts 读 contentWindow 时子窗 print
    // 已被替换。跨源子窗赋值抛错则保持原生 print 不动。
    try {
      const desc = Object.getOwnPropertyDescriptor(
        HTMLIFrameElement.prototype,
        'contentWindow',
      );
      if (desc && desc.get) {
        Object.defineProperty(HTMLIFrameElement.prototype, 'contentWindow', {
          configurable: true,
          get(this: HTMLIFrameElement) {
            const win = desc.get!.call(this) as (Window & { print?: () => void }) | null;
            if (win) {
              try {
                win.print = record;
              } catch {
                // cross-origin child: keep its native print
              }
            }
            return win;
          },
        });
      }
    } catch {
      // contentWindow non-configurable in some engines: the per-frame
      // init-script stub above already covers the same-origin case.
    }
  });
  await page.goto('/');
  await waitForDocument(page);

  const printViaSdk = async (): Promise<void> => {
    await page.evaluate(async () => {
      const ofd = (window as unknown as { __ofd?: OfdPrintApi }).__ofd;
      if (!ofd) {
        throw new Error('__ofd handle missing');
      }
      await ofd.print();
    });
  };

  await printViaSdk();
  const frames = page.locator('iframe[src^="blob:"]');
  await expect(frames).toHaveCount(1);
  const firstSrc = await frames.first().getAttribute('src');
  const firstFrame = await frames.first().elementHandle();
  expect(await page.evaluate(() => (window as unknown as { __printCalls: number }).__printCalls)).toBe(1);

  // 第二次打印：旧 iframe/URL 对被清理（存活帧恰 1 且 src 是新 blob URL，
  // 旧帧已脱离文档），print 再调一次。
  await printViaSdk();
  await expect(frames).toHaveCount(1);
  const secondSrc = await frames.first().getAttribute('src');
  expect(secondSrc).not.toBe(firstSrc);
  expect(await firstFrame!.evaluate((el) => el.isConnected)).toBe(false);
  expect(await page.evaluate(() => (window as unknown as { __printCalls: number }).__printCalls)).toBe(2);
});
