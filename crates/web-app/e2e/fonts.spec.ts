// 字体通道 e2e（Task 13）：首载预算 / legacy 回归缝 / 商用切片缺席降级。
//
// 记账方式与 brief 的差异（执行注记）：字体请求经 Playwright 网络层
// （page.on('response') + response.body().byteLength）而非 window.fetch
// init-script 包装，原因有二：
// 1. 切片 .woff2 由 font-runtime 的 FontWorker 在 worker 里 fetch
//    （fetcher.ts WorkerFetcher），window.fetch 包装根本看不见；
// 2. response.body() 给真实字节数，不依赖 dev 响应头是否带 content-length。
// 断言本身保持 brief 原样：无单条 >1MB、总量 <3MB、请求条数 ≤20。
import { expect, test, type Page } from '@playwright/test';

interface FontReq {
  url: string;
  bytes: number;
}

/** 字体类响应：woff2/otf/ttf/ttc（含查询串）。 */
const FONT_URL = /\.(woff2?|otf|ttf|ttc)(\?|$)/i;

/** 记录页面（含 FontWorker）发出的全部字体请求与真实字节数。 */
function trackFontRequests(page: Page): FontReq[] {
  const reqs: FontReq[] = [];
  page.on('response', (res) => {
    if (!FONT_URL.test(res.url())) {
      return;
    }
    // body() 在响应事件时可能未收完——异步补记；本地 dev 毫秒级，
    // 断言前的固定等待足够覆盖。
    void (async () => {
      const body = await res.body().catch(() => null);
      reqs.push({ url: res.url(), bytes: body === null ? 0 : body.byteLength });
    })();
  });
  return reqs;
}

test('首载无 >1MB 字体请求；总量 <3MB 且请求 ≤20（spec 验收）', async ({ page }) => {
  const reqs = trackFontRequests(page);
  await page.goto('/');
  await expect.poll(async () => reqs.length, { timeout: 15_000 }).toBeGreaterThan(0);
  await page.waitForTimeout(3_000); // 增量片到齐
  expect(reqs.every((r) => r.bytes <= 1_024 * 1_024), JSON.stringify(reqs, null, 1)).toBe(true);
  const total = reqs.reduce((n, r) => n + r.bytes, 0);
  expect(total, `total=${total}`).toBeLessThan(3 * 1_024 * 1_024);
  expect(reqs.length, JSON.stringify(reqs.map((r) => r.url))).toBeLessThanOrEqual(20);
});

test('?legacyFonts=1 走整字体通道（回归缝）', async ({ page }) => {
  const reqs = trackFontRequests(page);
  await page.goto('/?legacyFonts=1');
  await page.waitForTimeout(5_000); // 整字体下载
  expect(
    reqs.some((r) => /\.(ttf|ttc|otf)(\?|$)/i.test(r.url) && /\/fonts\//.test(r.url)),
    JSON.stringify(reqs.map((r) => r.url)),
  ).toBe(true);
});

test('商用切片目录缺席时降级不报错（新 clone / tauri 形态）', async ({ page }) => {
  const errors: string[] = [];
  page.on('pageerror', (e) => errors.push(String(e)));
  await page.goto('/');
  await page.waitForTimeout(2_000);
  expect(errors, errors.join('\n')).toEqual([]);
  const canvas = page.locator('canvas');
  await expect(canvas).toBeVisible();
});
