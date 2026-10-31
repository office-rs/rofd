// Print-flow e2e (print spec 2026-10-31 §7): exportPdf bytes / planPages
// range grammar / the iframe print channel (mount + window.print call +
// old-frame cleanup).
//
// Startup follows fonts.spec.ts (playwright.config boots the dev server,
// no extra stubs). The editor handle comes from the DEV-only test seam
// `window.__ofd` (App.vue, attached once Ofd.init resolves); the startup
// sequence then fetches public/sample.ofd and loadOfd, so the "document
// ready" predicate = planPages('All') resolving a non-empty page list
// (page_plan rejects a 0-page document with "document has no pages") —
// more reliable than a fixed wait.
//
// window.print stub accounting: printViaIframe calls the **iframe's
// contentWindow** .print(), not the main window's. addInitScript runs in
// every frame (on attach/navigation), so every frame gets the recorder;
// about:blank / blob: child frames are same-origin with the top, and the
// count lands on the main window via window.top. A contentWindow getter
// patch is added as a fallback for plugin-document commits (such
// documents never run the init script): each contentWindow access stamps
// the recorder onto the child window before returning it. Verified on
// this Chromium (probe-tested): the blob-PDF commits as a regular
// same-origin document already covered by the init script; the getter
// patch is only insurance against that behavior changing.
import { expect, test, type Page } from '@playwright/test';

/** The print surface of the SDK Ofd instance this spec consumes (dev
 * seam `window.__ofd`). */
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

/** Wait for the editor to be ready: `__ofd` attached and the document
 * loaded (non-empty All page list). */
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
    // Take only the prefix + length on the page side, avoiding a
    // whole-PDF serialize back into Node.
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
      // Custom range covering the grammar cap of 5000: out-of-bounds
      // pages are silently dropped (Word behavior), leaving exactly the
      // document's real pages — cross-checks the page count against All
      // via two independent code paths, no hardcoding.
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
    // Fallback: if the blob-PDF commits as a plugin document (no init
    // script), stamp at the contentWindow access boundary — by the time
    // print.ts reads contentWindow, the child's print is already
    // replaced. A cross-origin child throws on assignment; keep its
    // native print untouched.
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

  // Second print: the old iframe/URL pair is cleaned up (exactly 1 live
  // frame with a fresh blob URL src, the old frame detached from the
  // document), and print fires once more.
  await printViaSdk();
  await expect(frames).toHaveCount(1);
  const secondSrc = await frames.first().getAttribute('src');
  expect(secondSrc).not.toBe(firstSrc);
  expect(await firstFrame!.evaluate((el) => el.isConnected)).toBe(false);
  expect(await page.evaluate(() => (window as unknown as { __printCalls: number }).__printCalls)).toBe(2);
});
