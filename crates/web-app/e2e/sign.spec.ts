// Signing e2e (sign spec 2026-11-23 §10, §11): the real wasm in Chromium,
// through the app's own bundle. The unit tests stub the wasm surface, and
// native tests cannot see what only wasm32 shows - a wall-clock read
// (std::time is "not implemented" there) or a paint the CPU fallback
// renderer refuses (vello_cpu: opacity on an image draw) - so this spec is
// the regression guard for both, on the WebGPU path and on the CPU path.
//
// Startup follows print.spec.ts: playwright.config boots the dev server;
// the editor handle comes from the DEV seam `window.__ofd` (the SDK `Ofd`
// instance) and the module's signing functions from `window.__rofdSign`
// (App.vue). The sample document the app opens has no signature, which is
// where signing starts. The placement state is driven with real pointer
// events, so the SDK's DOM bindings are on the path too.
import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { chromium, expect, test, type Locator, type Page } from '@playwright/test';

const FIXTURES = fileURLToPath(new URL('../../sign/tests/fixtures/', import.meta.url));
/** ofdrw's throwaway test key file (password 777777). */
const USER_P12 = new Uint8Array(readFileSync(`${FIXTURES}USER.p12`));
const NOW = '2026-11-24 10:30:00';
const LAST_PAGE = { bottomRight: { pages: 'last', rightMm: 10, bottomMm: 10 } };

interface Report {
  overall: string;
  variant: string;
  kind: string;
}

interface Placement {
  pageIndex: number;
  pageId: string;
  boundary: { x: number; y: number; w: number; h: number };
}

interface SignOptionsWire {
  seal?: unknown;
  placement?: unknown;
  now: string;
}

/** What this spec uses of the DEV seams. `__ofd` is the SDK instance. */
interface Seam {
  __ofd?: {
    loadOfd(bytes: Uint8Array): void;
    /** The instance signs the document that is open (spec §17.11). */
    sign(pfx: Uint8Array, password: string, options: SignOptionsWire): Promise<Report[]>;
    isSignBlocked(): boolean;
    saveOfd(): Uint8Array;
    isSaveBlockedBySignature(): boolean;
    sealPictures(): (Uint8Array | null)[];
    beginSealPlacement(png: Uint8Array, widthMm: number, heightMm: number): boolean;
    setOnSealPlaced(cb: (placement: Placement) => void): void;
    readonly isCpuFallback: boolean;
  };
  __rofdSign?: {
    verifySignatures(ofd: Uint8Array, options?: object): Promise<Report[]>;
    signOfd(
      ofd: Uint8Array,
      pfx: Uint8Array,
      password: string,
      options: SignOptionsWire,
    ): Promise<Uint8Array>;
    certificateInfo(pfx: Uint8Array, password: string): Promise<{ label: string }>;
  };
  __placed?: Placement | null;
}

/** Wait for the editor and the document (the sample) to be ready. */
async function waitForDocument(page: Page): Promise<void> {
  await page.waitForFunction(
    () => {
      const seam = window as unknown as Seam;
      return seam.__ofd !== undefined && seam.__rofdSign !== undefined;
    },
    null,
    { timeout: 20_000 },
  );
  await expect
    .poll(() =>
      page.evaluate(() => (window as unknown as Seam).__ofd!.saveOfd().length > 0),
    )
    .toBe(true);
}

/** Every wasm panic on the page, from here on. */
function watchPanics(page: Page): () => string[] {
  const panics: string[] = [];
  page.on('console', (msg) => {
    const text = msg.text();
    if (msg.type() === 'error' && /panicked at|RuntimeError: unreachable/.test(text)) {
      panics.push(text.slice(0, 200));
    }
  });
  page.on('pageerror', (error) => panics.push(`pageerror: ${String(error).slice(0, 200)}`));
  return () => panics;
}

function verdicts(reports: Report[]): string[] {
  return reports.map((report) => `${report.overall}/${report.variant}/${report.kind}`);
}

async function currentDocument(page: Page): Promise<Uint8Array> {
  return page.evaluate(() => (window as unknown as Seam).__ofd!.saveOfd());
}

async function verify(page: Page, ofd: Uint8Array): Promise<string[]> {
  return page.evaluate(async (ofd) => {
    const sign = (window as unknown as Seam).__rofdSign!;
    const reports = await sign.verifySignatures(ofd, {});
    return reports.map((report) => `${report.overall}/${report.variant}`);
  }, ofd);
}

async function isCpuFallback(page: Page): Promise<boolean> {
  return page.evaluate(() => (window as unknown as Seam).__ofd!.isCpuFallback);
}

/** Sign the page's own document with a built-in seal picture, auto-placed,
 * so that it carries a seal; the page then shows the signed document. */
async function signWithSeal(page: Page): Promise<void> {
  const pictures = await page.evaluate(
    async ({ pfx, now, placement }) => {
      const ofd = (window as unknown as Seam).__ofd!;
      const sign = (window as unknown as Seam).__rofdSign!;
      const response = await fetch('/seals/round.png');
      const png = new Uint8Array(await response.arrayBuffer());
      const options = { seal: { png, sizeMm: 40 }, placement, now };
      ofd.loadOfd(await sign.signOfd(ofd.saveOfd(), pfx, '777777', options));
      return ofd.sealPictures().length;
    },
    { pfx: USER_P12, now: NOW, placement: LAST_PAGE },
  );
  expect(pictures).toBe(1);
}

/** Where the pointer goes on the page (the canvas fills the viewport below
 * the ribbon; the sample's page is centred). */
const ON_PAGE = { x: 700, y: 520 } as const;
/** The part of the page that the preview's move changes. */
const CLIP = { x: 480, y: 380, width: 440, height: 280 } as const;

/** Enter the placement state, move the pointer on the page, and click: the
 * preview must paint (the page changes) and the placement must be
 * reported. */
async function placeSeal(page: Page): Promise<{ painted: boolean; placement: Placement | null }> {
  const entered = await page.evaluate(() => {
    const seam = window as unknown as Seam;
    seam.__placed = null;
    const ofd = seam.__ofd!;
    const picture = ofd.sealPictures()[0];
    if (picture === null || picture === undefined) {
      throw new Error('the signed document carries no seal picture');
    }
    ofd.setOnSealPlaced((placement) => {
      seam.__placed = placement;
    });
    return ofd.beginSealPlacement(picture, 40, 40);
  });
  expect(entered).toBe(true);

  await page.mouse.move(ON_PAGE.x - 200, ON_PAGE.y - 200);
  await page.waitForTimeout(300);
  const before = await page.screenshot({ clip: CLIP });
  await page.mouse.move(ON_PAGE.x, ON_PAGE.y, { steps: 4 });
  await page.waitForTimeout(400);
  const preview = await page.screenshot({ clip: CLIP });

  await page.mouse.click(ON_PAGE.x, ON_PAGE.y);
  await page.waitForTimeout(400);
  const placement = await page.evaluate(() => (window as unknown as Seam).__placed);
  return { painted: !preview.equals(before), placement };
}

function expectPlaced(result: { painted: boolean; placement: Placement | null }): void {
  expect(result.painted).toBe(true);
  expect(result.placement).toMatchObject({ pageIndex: 0, pageId: expect.any(String) });
  expect(result.placement?.boundary.w).toBeCloseTo(40, 3);
  expect(result.placement?.boundary.h).toBeCloseTo(40, 3);
}

test('a certificate file signs the document and the result verifies', async ({ page }) => {
  const panics = watchPanics(page);
  await page.goto('/');
  await waitForDocument(page);

  // The sample has no signature: an empty array, never an error.
  const document = await currentDocument(page);
  expect(await verify(page, document)).toEqual([]);

  const result = await page.evaluate(
    async ({ document, pfx, now, placement }) => {
      const sign = (window as unknown as Seam).__rofdSign!;
      let wrongPassword: string | null = null;
      try {
        await sign.certificateInfo(pfx, 'wrong');
      } catch (e) {
        wrongPassword = String(e);
      }
      const info = await sign.certificateInfo(pfx, '777777');
      const signed = await sign.signOfd(document, pfx, '777777', { seal: null, placement, now });
      return {
        wrongPassword,
        info,
        signed: Array.from(signed),
        reports: await sign.verifySignatures(signed, {}),
      };
    },
    { document, pfx: USER_P12, now: NOW, placement: LAST_PAGE },
  );
  // A wrong password is an error with a reason; the right one reads the file.
  expect(result.wrongPassword).toMatch(/password/);
  expect(result.info.label).toBe('Test Certificate');
  expect(verdicts(result.reports)).toEqual(['Valid/Gbt35275/Sign']);

  // A save of the signed document keeps the signature valid (the
  // preserving save rewrites only the annotation entries).
  const saved = await page.evaluate(async (signed) => {
    const ofd = (window as unknown as Seam).__ofd!;
    ofd.loadOfd(new Uint8Array(signed));
    return Array.from(ofd.saveOfd());
  }, result.signed);
  expect(await verify(page, new Uint8Array(saved))).toEqual(['Valid/Gbt35275']);
  expect(panics()).toEqual([]);
});

test('the seal preview paints on the WebGPU renderer and the click reports the placement', async ({
  page,
}) => {
  const panics = watchPanics(page);
  await page.goto('/');
  await waitForDocument(page);
  expect(await isCpuFallback(page)).toBe(false);
  await signWithSeal(page);
  expect(await verify(page, await currentDocument(page))).toEqual(['Valid/SesV4']);
  expectPlaced(await placeSeal(page));
  expect(panics()).toEqual([]);
});

test('the seal preview paints on the CPU fallback renderer too', async () => {
  // A browser without a GPU stack: the app lands in the CPU fallback, whose
  // renderer refuses what the WebGPU one accepts (opacity on an image draw).
  const browser = await chromium.launch({
    executablePath: chromium.executablePath(),
    args: ['--disable-gpu'],
  });
  try {
    const base = test.info().project.use.baseURL ?? 'http://localhost:5190';
    const page = await browser.newPage({ viewport: { width: 1280, height: 900 } });
    const panics = watchPanics(page);
    await page.goto(`${base}/`);
    await waitForDocument(page);
    expect(await isCpuFallback(page)).toBe(true);
    await signWithSeal(page);
    expect(await verify(page, await currentDocument(page))).toEqual(['Valid/SesV4']);
    expectPlaced(await placeSeal(page));
    expect(panics()).toEqual([]);
  } finally {
    await browser.close();
  }
});

/** How many reddish pixels the editor canvas shows. The CPU fallback paints
 * into a 2D canvas, so they can be counted; the seal is the only red the
 * sample's last page gains. */
function redPixels(): number {
  const canvas = [...document.querySelectorAll('canvas')].sort(
    (a, b) => b.width * b.height - a.width * a.height,
  )[0];
  const context = canvas.getContext('2d');
  if (context === null) {
    throw new Error('not a 2D canvas: this is not the CPU fallback');
  }
  const { data } = context.getImageData(0, 0, canvas.width, canvas.height);
  let red = 0;
  for (let i = 0; i < data.length; i += 4) {
    if (data[i + 3] > 0 && data[i] - data[i + 1] > 60 && data[i] - data[i + 2] > 60) {
      red += 1;
    }
  }
  return red;
}

test('a signature signed in place shows its seal on the CPU fallback renderer', async () => {
  // The CPU path keeps a bitmap per page, keyed on the kernel's body
  // revision. A signature moves it (the seal is page content), or the
  // cached bitmap would go on without the seal.
  const browser = await chromium.launch({
    executablePath: chromium.executablePath(),
    args: ['--disable-gpu'],
  });
  try {
    const base = test.info().project.use.baseURL ?? 'http://localhost:5190';
    const page = await browser.newPage({ viewport: { width: 1280, height: 900 } });
    const panics = watchPanics(page);
    await page.goto(`${base}/`);
    await waitForDocument(page);
    expect(await isCpuFallback(page)).toBe(true);

    // The seal goes to the bottom right of the last page: scroll there.
    await page.mouse.move(ON_PAGE.x, ON_PAGE.y);
    await page.mouse.wheel(0, 5000);
    await expect(page.getByText(/第 \d+ 页/)).toHaveText('第 2 页');
    await page.waitForTimeout(600);
    const before = await page.evaluate(redPixels);

    // What the dialog does: one call, the signature goes into the open
    // document and the reports come back.
    const { blocked, reports } = await page.evaluate(
      async ({ pfx, now, placement }) => {
        const seam = window as unknown as Seam;
        const ofd = seam.__ofd!;
        const png = new Uint8Array(
          await (await fetch('/seals/round.png')).arrayBuffer(),
        );
        const blocked = ofd.isSignBlocked();
        const reports = blocked
          ? []
          : await ofd.sign(pfx, '777777', {
              seal: { png, sizeMm: 40 },
              placement,
              now,
            });
        return { blocked, reports };
      },
      { pfx: USER_P12, now: NOW, placement: LAST_PAGE },
    );
    expect(blocked).toBe(false);
    expect(reports.map((report) => report.overall)).toEqual(['Valid']);
    await page.waitForTimeout(600);
    const after = await page.evaluate(redPixels);

    // The seal is on the page that was already drawn, and the view stayed.
    expect(after - before).toBeGreaterThan(500);
    await expect(page.getByText(/第 \d+ 页/)).toHaveText('第 2 页');
    expect(await verify(page, await currentDocument(page))).toEqual(['Valid/SesV4']);
    expect(panics()).toEqual([]);
  } finally {
    await browser.close();
  }
});

/** Drive the signing dialog: import the test key file, take the certificate
 * it holds, and sign with the dialog's own choice of picture. */
async function signThroughDialog(
  page: Page,
  chooseSeal: (dialog: Locator) => Promise<void>,
): Promise<void> {
  await page.locator('.tb-tab', { hasText: '签章' }).click();
  await page
    .locator('.tb-row:visible .tool-btn:not([disabled])', { hasText: '签章' })
    .click();
  const dialog = page.locator('.ant-modal', { hasText: '签章' });
  await expect(dialog).toBeVisible();
  await dialog.getByRole('button', { name: '导入证书…' }).click();
  await dialog
    .locator('input[type="file"][accept=".pfx,.p12"]')
    .setInputFiles(`${FIXTURES}USER.p12`);
  await dialog.getByPlaceholder('证书口令').fill('777777');
  // antd puts a space between the two characters of a short button label.
  await dialog.getByRole('button', { name: '导 入', exact: true }).click();
  // The import selects the certificate and carries the password over.
  await expect(dialog.locator('.ant-select-selection-item')).toContainText(
    'Test Certificate',
  );
  await chooseSeal(dialog);
  await dialog.locator('.ant-btn-primary', { hasText: '确 定' }).click();
}

test('signing a signed file that has no annotation entry is refused', async ({ page }) => {
  // ofdrw's signed sample declares no annotation entry; the fixture adds one
  // annotation in the old layout (a page file, no entry file). The
  // preserving save cannot write it, so the host must not sign what the
  // save returned: that would put the file back without the annotation
  // under "签章完成" (final review I2).
  const annotated = Array.from(readFileSync(`${FIXTURES}SESV4SignDocAnnotated.ofd`));
  const downloads: string[] = [];
  page.on('download', (download) => downloads.push(download.suggestedFilename()));
  await page.goto('/');
  await waitForDocument(page);
  // The flags describe the last save, as the host reads them: the bytes
  // first, then the flags.
  const blocked = await page.evaluate(async (bytes) => {
    const ofd = (window as unknown as Seam).__ofd!;
    ofd.loadOfd(new Uint8Array(bytes));
    ofd.saveOfd();
    return ofd.isSaveBlockedBySignature();
  }, annotated);
  expect(blocked).toBe(true);

  await signThroughDialog(page, (dialog) =>
    dialog.getByTitle('不使用章图：直接电子签名').click(),
  );
  await expect(page.locator('.ant-message')).toContainText('没有批注入口');
  // Nothing was written, and the document in the editor is as it was.
  await page.waitForTimeout(800);
  expect(downloads).toEqual([]);
  expect(
    await page.evaluate(() => {
      const ofd = (window as unknown as Seam).__ofd!;
      ofd.saveOfd();
      return ofd.isSaveBlockedBySignature();
    }),
  ).toBe(true);
});

test('signing updates the document; the file is written when the user saves', async ({
  page,
}) => {
  // flw, 2026-11-24: signing used to download the signed package at once.
  // The signature goes into the document on screen; the file is written by
  // the user's save, and until then the status bar says it is not saved.
  const downloads: string[] = [];
  page.on('download', (download) => downloads.push(download.suggestedFilename()));
  await page.goto('/');
  await waitForDocument(page);

  // The user is on the second page (the sample has two) when the document
  // is signed, and nudges the view afterwards.
  const pageLine = page.getByText(/第 \d+ 页/);
  await page.mouse.move(ON_PAGE.x, ON_PAGE.y);
  await page.mouse.wheel(0, 2000);
  await expect(pageLine).toHaveText('第 2 页');

  await signThroughDialog(page, (dialog) => dialog.getByRole('checkbox').check());
  // The document on screen carries the signature; nothing was written.
  await expect(page.locator('.sb-sign')).toContainText('签章：全部有效（1）');
  await expect(page.locator('.sb-sign-unsaved')).toContainText('签章未保存');
  await page.waitForTimeout(800);
  expect(downloads).toEqual([]);
  expect(
    await page.evaluate(
      () => (window as unknown as Seam).__ofd!.sealPictures().length,
    ),
  ).toBe(1);

  // Looking at the signature that was just made: the detail opens on what
  // the document carries. The file does not have it yet, so the status bar
  // must not be read from the file again (it used to lose the signature).
  await page.locator('.sb-sign').click();
  const detail = page.locator('.ant-modal', { hasText: '签章详情' });
  await expect(detail).toBeVisible();
  await expect(detail.locator('.sgd-item')).toHaveCount(1);
  await expect(detail).not.toContainText('该文档没有签章');
  await detail.locator('.ant-modal-close').click();
  await expect(detail).toBeHidden();
  await expect(page.locator('.sb-sign')).toContainText('签章：全部有效（1）');
  await expect(page.locator('.sb-sign-unsaved')).toContainText('签章未保存');
  // The signature went into the document that was open: the view did not
  // go back to the first page, as it did when the signed package was loaded
  // as a new document.
  await page.mouse.move(ON_PAGE.x, ON_PAGE.y);
  await page.mouse.wheel(0, 2);
  await expect(pageLine).toHaveText('第 2 页');

  // The user saves: the signed package is written, once.
  await page.mouse.click(ON_PAGE.x, ON_PAGE.y);
  await page.keyboard.press('Control+s');
  await expect.poll(() => downloads).toEqual(['document.ofd']);
  await expect(page.locator('.sb-sign-unsaved')).toHaveCount(0);
  await expect(page.locator('.sb-sign')).toContainText('签章：全部有效（1）');
});

