import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import {
  Ofd,
  PreparedSignature,
  __testSetSignLoader,
  __testSetSignModule,
  certificateInfo,
  signOfd,
  signPrepare,
  verifySignatures,
} from '../index';
import { RafStub } from './helpers';

type WasmSurface = Parameters<typeof Ofd.__testNew>[0];

function makeOfd(): { ofd: Ofd; wasm: WasmSurface } {
  const wasm = { renderFrame: (): number => -1 } as unknown as WasmSurface;
  const ofd = Ofd.__testNew(wasm, document.createElement('canvas'));
  return { ofd, wasm: (ofd as unknown as { wasm: WasmSurface }).wasm };
}

describe('seal placement wiring', () => {
  let raf: RafStub;

  beforeEach(() => {
    vi.useFakeTimers();
    raf = new RafStub();
    raf.install();
  });

  afterEach(() => {
    vi.useRealTimers();
    vi.unstubAllGlobals();
  });

  it('beginSealPlacement passes png, widthMm, heightMm through and returns the kernel flag', () => {
    const kernel = { entered: true };
    const calls: unknown[][] = [];
    const wasm = {
      renderFrame: (): number => -1,
      beginSealPlacement: (...args: unknown[]): boolean => {
        calls.push(args);
        return kernel.entered;
      },
      cancelSealPlacement: vi.fn(),
    };
    const { ofd } = (() => {
      const made = Ofd.__testNew(wasm as unknown as WasmSurface, document.createElement('canvas'));
      return { ofd: made };
    })();
    try {
      const png = new Uint8Array([137, 80, 78, 71, 13, 10, 26, 10]);
      expect(ofd.beginSealPlacement(png, 40, 20)).toBe(true);
      expect(calls).toEqual([[png, 40, 20]]);
      expect(raf.count).toBe(1, 'like any wasm call, it kicks one frame');

      // An undecodable picture is refused by the kernel, and the SDK says so.
      kernel.entered = false;
      expect(ofd.beginSealPlacement(new Uint8Array([1]), 40, 40)).toBe(false);

      ofd.cancelSealPlacement();
      expect(wasm.cancelSealPlacement).toHaveBeenCalledTimes(1);
    } finally {
      ofd.destroy();
    }
  });

  it('setOnSealPlaced registers the callback and forwards the payload', async () => {
    const slots: Record<string, ((...args: unknown[]) => void) | null> = {};
    const wasm = {
      renderFrame: (): number => -1,
      setOnSealPlaced: (cb: ((...args: unknown[]) => void) | null): void => {
        slots.placed = cb;
      },
      setOnSealPlacementCancelled: (cb: (() => void) | null): void => {
        slots.cancelled = cb;
      },
    };
    const ofd = Ofd.__testNew(wasm as unknown as WasmSurface, document.createElement('canvas'));
    try {
      const seen: unknown[] = [];
      ofd.setOnSealPlaced((placement) => seen.push(placement));
      expect(slots.placed).toBeTypeOf('function');
      slots.placed?.({ pageIndex: 0, pageId: 'P0', boundary: { x: 80, y: 80, w: 40, h: 40 } });
      // The handler runs in a microtask, so it may call back into wasm.
      expect(seen).toEqual([]);
      await Promise.resolve();
      expect(seen).toEqual([
        { pageIndex: 0, pageId: 'P0', boundary: { x: 80, y: 80, w: 40, h: 40 } },
      ]);

      const cancelled = vi.fn();
      ofd.setOnSealPlacementCancelled(cancelled);
      slots.cancelled?.();
      expect(cancelled).not.toHaveBeenCalled();
      await Promise.resolve();
      expect(cancelled).toHaveBeenCalledTimes(1);

      // null removes a callback.
      ofd.setOnSealPlaced(null);
      ofd.setOnSealPlacementCancelled(null);
      expect(slots.placed).toBeNull();
      expect(slots.cancelled).toBeNull();
    } finally {
      ofd.destroy();
    }
  });

  it('sign passes the certificate file, the password and the options through and returns the reports', async () => {
    const reports = [{ id: '1', overall: 'Valid' }];
    const wasm = {
      renderFrame: (): number => -1,
      sign: vi.fn((): string => JSON.stringify(reports)),
      isSignBlocked: (): boolean => false,
    };
    const ofd = Ofd.__testNew(wasm as unknown as WasmSurface, document.createElement('canvas'));
    try {
      const pfx = new Uint8Array([1, 2]);
      const options = { seal: null, now: '2026-11-24 10:30:00' };
      expect(await ofd.sign(pfx, '777777', options)).toEqual(reports);
      // The wire form is the SDK's: seal, placement, now.
      expect(wasm.sign).toHaveBeenCalledWith(
        pfx,
        '777777',
        JSON.stringify({ seal: null, placement: null, now: '2026-11-24 10:30:00' }),
      );
      expect(ofd.isSignBlocked()).toBe(false);
      expect(raf.count).toBe(1, 'the seal shows at the next frame');
    } finally {
      ofd.destroy();
    }
  });

  it('verifySignatures on the instance verifies the open document', async () => {
    const reports = [{ id: '1', overall: 'Valid' }];
    const wasm = {
      renderFrame: (): number => -1,
      verifySignatures: vi.fn((): string => JSON.stringify(reports)),
    };
    const ofd = Ofd.__testNew(wasm as unknown as WasmSurface, document.createElement('canvas'));
    try {
      // No bytes to hand in: the document is the one that is open.
      expect(await ofd.verifySignatures()).toEqual(reports);
      expect(wasm.verifySignatures).toHaveBeenLastCalledWith('{}');
      // The options cross as the module function's do.
      await ofd.verifySignatures({ now: '2026-11-24 10:30:00' });
      expect(wasm.verifySignatures).toHaveBeenLastCalledWith(
        JSON.stringify({ now: '2026-11-24 10:30:00' }),
      );
      await ofd.verifySignatures({ certs: { '1': new Uint8Array([4, 5]) } });
      expect(wasm.verifySignatures).toHaveBeenLastCalledWith(
        JSON.stringify({ certs: { '1': 'BAU=' } }),
      );
    } finally {
      ofd.destroy();
    }
  });

  it('prepareSign stays on the instance: apply lands the signature through finishSign', () => {
    const inner = { toSign: (): Uint8Array => new Uint8Array([7]) };
    const reports = [{ id: '1', overall: 'Valid' }];
    const finishSign = vi.fn(
      (): { done: boolean; reports: string | null; next: () => undefined } => ({
        done: true,
        reports: JSON.stringify(reports),
        next: (): undefined => undefined,
      }),
    );
    const wasm = { renderFrame: (): number => -1, prepareSign: vi.fn(() => inner), finishSign };
    const ofd = Ofd.__testNew(wasm as unknown as WasmSurface, document.createElement('canvas'));
    try {
      const step = ofd.prepareSign(new Uint8Array([3]), { seal: null, now: '2026-11-24 10:30:00' });
      expect(step.toSign()).toEqual(new Uint8Array([7]));
      // The signature is in the document, and the reports came back with it.
      expect(step.apply(new Uint8Array([5]))).toEqual({ done: true, next: null, reports });
      expect(finishSign).toHaveBeenCalledWith(inner, new Uint8Array([5]));
    } finally {
      ofd.destroy();
    }
  });

  it('a second round comes back as the next step', () => {
    const second = { toSign: (): Uint8Array => new Uint8Array([8]) };
    const finishSign = vi
      .fn()
      .mockReturnValueOnce({ done: false, reports: null, next: () => second })
      .mockReturnValueOnce({ done: true, reports: '[]', next: () => undefined });
    const wasm = {
      renderFrame: (): number => -1,
      prepareSign: vi.fn(() => ({ toSign: () => new Uint8Array([7]) })),
      finishSign,
    };
    const ofd = Ofd.__testNew(wasm as unknown as WasmSurface, document.createElement('canvas'));
    try {
      const next = ofd
        .prepareSign(new Uint8Array([3]), { seal: null, now: '2026-11-24 10:30:00' })
        .apply(new Uint8Array([5])).next;
      expect(next).toBeInstanceOf(PreparedSignature);
      expect(next!.apply(new Uint8Array([6]))).toEqual({ done: true, next: null, reports: [] });
      expect(finishSign).toHaveBeenLastCalledWith(second, new Uint8Array([6]));
    } finally {
      ofd.destroy();
    }
  });

  it('a PreparedSignature is good for one apply', () => {
    const inner = { toSign: (): Uint8Array => new Uint8Array([7]) };
    const finishSign = vi.fn(() => ({ done: true, reports: '[]', next: () => undefined }));
    const wasm = { renderFrame: (): number => -1, prepareSign: vi.fn(() => inner), finishSign };
    const ofd = Ofd.__testNew(wasm as unknown as WasmSurface, document.createElement('canvas'));
    try {
      const step = ofd.prepareSign(new Uint8Array([3]), { seal: null, now: '2026-11-24 10:30:00' });
      expect(step.apply(new Uint8Array([5])).done).toBe(true);
      // The round is spent. A second apply is stopped here, with a message
      // of rofd's own: wasm has consumed the object and would answer a
      // null-pointer error.
      expect(() => step.apply(new Uint8Array([6]))).toThrow(/already used/);
      expect(() => step.toSign()).toThrow(/already used/);
      expect(finishSign).toHaveBeenCalledTimes(1);
    } finally {
      ofd.destroy();
    }
  });

  it('a refused round is spent too: the retry does not reach wasm again', () => {
    // A token or a remote signer hands back a signature that is refused; the
    // host sends a corrected one - on the same object.
    const inner = { toSign: (): Uint8Array => new Uint8Array([7]) };
    const finishSign = vi
      .fn()
      .mockImplementationOnce((): never => {
        throw new Error('the signature does not verify with the signer certificate');
      })
      .mockReturnValue({ done: true, reports: '[]', next: () => undefined });
    const wasm = { renderFrame: (): number => -1, prepareSign: vi.fn(() => inner), finishSign };
    const ofd = Ofd.__testNew(wasm as unknown as WasmSurface, document.createElement('canvas'));
    try {
      const step = ofd.prepareSign(new Uint8Array([3]), { seal: null, now: '2026-11-24 10:30:00' });
      expect(() => step.apply(new Uint8Array([5]))).toThrow('does not verify');
      // wasm took the round with the refused answer. The retry is stopped by
      // the SDK, not answered by wasm with a null-pointer error.
      expect(() => step.apply(new Uint8Array([6]))).toThrow(/already used/);
      expect(() => step.toSign()).toThrow(/already used/);
      expect(finishSign).toHaveBeenCalledTimes(1);
    } finally {
      ofd.destroy();
    }
  });

  it('a finished step that carries no reports is an error, not "no signatures"', () => {
    const inner = { toSign: (): Uint8Array => new Uint8Array([7]) };
    // Older glue, or a bug: the signature is in the document and no reports
    // came with it.
    const finishSign = vi.fn(() => ({ done: true, reports: null, next: () => undefined }));
    const wasm = { renderFrame: (): number => -1, prepareSign: vi.fn(() => inner), finishSign };
    const ofd = Ofd.__testNew(wasm as unknown as WasmSurface, document.createElement('canvas'));
    try {
      const step = ofd.prepareSign(new Uint8Array([3]), { seal: null, now: '2026-11-24 10:30:00' });
      expect(() => step.apply(new Uint8Array([5]))).toThrow(/without reports/);
    } finally {
      ofd.destroy();
    }
  });

  it('saveOfd reports what the save had to do, and offers the full write', () => {
    const flags = { blocked: true, reserve: false, fallback: false };
    const fullWrite = vi.fn((): Uint8Array => new Uint8Array([9]));
    const wasm = {
      renderFrame: (): number => -1,
      saveOfd: (): Uint8Array => new Uint8Array([1]),
      isSaveBlockedBySignature: (): boolean => flags.blocked,
      isSaveReserveExhausted: (): boolean => flags.reserve,
      isSaveFullWriteFallback: (): boolean => flags.fallback,
      saveOfdFullWrite: fullWrite,
    };
    const ofd = Ofd.__testNew(wasm as unknown as WasmSurface, document.createElement('canvas'));
    try {
      expect(ofd.saveOfd()).toEqual(new Uint8Array([1]));
      expect(ofd.isSaveBlockedBySignature()).toBe(true);
      expect(ofd.isSaveReserveExhausted()).toBe(false);
      expect(ofd.isSaveFullWriteFallback()).toBe(false);
      // Only after the host asked the user: the full write invalidates the
      // signatures of a package without an annotation entry.
      expect(fullWrite).not.toHaveBeenCalled();
      expect(ofd.saveOfdFullWrite()).toEqual(new Uint8Array([9]));
      flags.blocked = false;
      flags.reserve = true;
      expect(ofd.isSaveBlockedBySignature()).toBe(false);
      expect(ofd.isSaveReserveExhausted()).toBe(true);
    } finally {
      ofd.destroy();
    }
  });
});

describe('module-level signing functions', () => {
  let raf: RafStub;

  beforeEach(() => {
    vi.useFakeTimers();
    raf = new RafStub();
    raf.install();
  });

  afterEach(() => {
    vi.useRealTimers();
    vi.unstubAllGlobals();
    __testSetSignModule(null);
    __testSetSignLoader(null);
  });

  function stubModule() {
    const mod = {
      verifySignatures: vi.fn(
        (_ofd: Uint8Array, _options: string): string => '[{"id":"1","overall":"Valid"}]',
      ),
      signOfd: vi.fn((): Uint8Array => new Uint8Array([1, 2, 3])),
      certificateInfo: vi.fn(
        (): string => '{"id":"ab","label":"Test Certificate","subject_cn":"Test Certificate"}',
      ),
      signPrepare: vi.fn(),
    };
    __testSetSignModule(mod as never);
    return mod;
  }

  it('signOfd and verifySignatures call the module functions without kicking the scheduler', async () => {
    const mod = stubModule();
    // An instance in the suspended state: the pure functions must not wake it.
    const { ofd } = makeOfd();
    try {
      ofd.kick();
      raf.flushOne();
      expect(raf.count).toBe(0);

      const bytes = new Uint8Array([1, 2]);
      const reports = await verifySignatures(bytes, { now: '2026-11-24 10:30:00' });
      expect(mod.verifySignatures).toHaveBeenCalledWith(
        bytes,
        JSON.stringify({ now: '2026-11-24 10:30:00' }),
      );
      expect(reports).toEqual([{ id: '1', overall: 'Valid' }]);

      const pfx = new Uint8Array([3]);
      const signed = await signOfd(pfx, pfx, 'secret', {
        now: '2026-11-24 10:30:00',
        seal: { png: bytes, sizeMm: 40 },
        placement: { at: { page: 2, boundary: { x: 10, y: 20, w: 40, h: 30 } } },
      });
      expect(signed).toEqual(new Uint8Array([1, 2, 3]));
      expect(mod.signOfd).toHaveBeenCalledWith(
        pfx,
        pfx,
        'secret',
        JSON.stringify({
          seal: { png: [1, 2], sizeMm: 40 },
          placement: { at: { page: 2, boundary: { x: 10, y: 20, w: 40, h: 30 } } },
          now: '2026-11-24 10:30:00',
        }),
      );

      // Without options: the defaults of the wasm side.
      await verifySignatures(bytes);
      expect(mod.verifySignatures).toHaveBeenLastCalledWith(bytes, JSON.stringify({}));
      await verifySignatures(bytes, { certs: { '1': new Uint8Array([4, 5]) } });
      expect(mod.verifySignatures).toHaveBeenLastCalledWith(
        bytes,
        JSON.stringify({ certs: { '1': 'BAU=' } }),
      );

      expect(raf.count).toBe(0, 'no frame was scheduled');
    } finally {
      ofd.destroy();
    }
  });

  it('certificateInfo reads a certificate file for display', async () => {
    const mod = stubModule();
    const pfx = new Uint8Array([7]);
    const entry = await certificateInfo(pfx, 'secret');
    expect(mod.certificateInfo).toHaveBeenCalledWith(pfx, 'secret');
    expect(entry.label).toBe('Test Certificate');
  });

  it('signPrepare wraps the rounds of a signer outside the process', async () => {
    const mod = stubModule();
    const bytes = new Uint8Array([1]);
    const second = {
      toSign: (): Uint8Array => new Uint8Array([200]),
      apply: vi.fn(
        (): {
          done: boolean;
          bytes: Uint8Array;
          next: () => undefined;
        } => ({
          done: true,
          bytes: new Uint8Array([9, 9]),
          next: (): undefined => undefined,
        }),
      ),
    };
    const first = {
      toSign: (): Uint8Array => new Uint8Array([100]),
      apply: vi.fn(
        (): {
          done: boolean;
          bytes: Uint8Array;
          next: () => typeof second;
        } => ({ done: false, bytes: new Uint8Array(), next: () => second }),
      ),
    };
    mod.signPrepare.mockReturnValue(first);

    const prepared = await signPrepare(bytes, new Uint8Array([5]), { now: '2026-11-24 10:30:00' });
    expect(mod.signPrepare).toHaveBeenCalledWith(
      bytes,
      new Uint8Array([5]),
      JSON.stringify({ seal: null, placement: null, now: '2026-11-24 10:30:00' }),
    );
    expect(prepared.toSign()).toEqual(new Uint8Array([100]));

    // A seal takes two rounds: the first yields the next one.
    const step = prepared.apply(new Uint8Array([42]));
    expect(first.apply).toHaveBeenCalledWith(new Uint8Array([42]));
    expect(step.done).toBe(false);
    expect(step.bytes).toBeNull();
    expect(step.next).not.toBeNull();
    // The second yields the package.
    const secondRound = step.next as InstanceType<typeof PreparedSignOfClass>;
    const last = secondRound.apply(new Uint8Array([43]));
    expect(second.apply).toHaveBeenCalledWith(new Uint8Array([43]));
    expect(last.done).toBe(true);
    expect(last.bytes).toEqual(new Uint8Array([9, 9]));
    expect(last.next).toBeNull();
    // Each round is good for one apply, on this side of the line too.
    expect(() => secondRound.apply(new Uint8Array([44]))).toThrow(/already used/);
    expect(() => secondRound.toSign()).toThrow(/already used/);
    expect(() => prepared.apply(new Uint8Array([44]))).toThrow(/already used/);
    expect(second.apply).toHaveBeenCalledTimes(1);
  });

  it('a refused round of a package is spent too', async () => {
    const mod = stubModule();
    const first = {
      toSign: (): Uint8Array => new Uint8Array([100]),
      apply: vi
        .fn()
        .mockImplementationOnce((): never => {
          throw new Error('refused');
        })
        .mockReturnValue({ done: true, bytes: new Uint8Array([9]), next: () => undefined }),
    };
    mod.signPrepare.mockReturnValue(first);
    const prepared = await signPrepare(new Uint8Array([1]), new Uint8Array([5]), {
      now: '2026-11-24 10:30:00',
    });

    expect(() => prepared.apply(new Uint8Array([42]))).toThrow('refused');
    // wasm took the round with the refused answer; the retry is stopped here.
    expect(() => prepared.apply(new Uint8Array([43]))).toThrow(/already used/);
    expect(() => prepared.toSign()).toThrow(/already used/);
    expect(first.apply).toHaveBeenCalledTimes(1);
  });
});

describe('the signing module', () => {
  it('a failed load is not cached: the next call loads again', async () => {
    // The module is fetched the first time it is needed. A page that saw
    // one failed fetch must not stay without signing until it is reloaded
    // (final review I6).
    __testSetSignModule(null);
    __testSetSignLoader(null);
    const mod = {
      verifySignatures: vi.fn((): string => '[{"id":"1","overall":"Valid"}]'),
    };
    let attempts = 0;
    __testSetSignLoader(() => {
      attempts += 1;
      return attempts === 1
        ? Promise.reject(new Error('chunk failed to load'))
        : Promise.resolve(mod as never);
    });
    const bytes = new Uint8Array([1, 2]);
    await expect(verifySignatures(bytes)).rejects.toThrow('chunk failed to load');
    await expect(verifySignatures(bytes)).resolves.toEqual([{ id: '1', overall: 'Valid' }]);
    expect(attempts).toBe(2);
    // Loaded once, shared afterwards.
    await verifySignatures(bytes);
    expect(attempts).toBe(2);
    __testSetSignLoader(null);
    __testSetSignModule(null);
  });
});

// The class is exported for `instanceof`; named here so the test reads clearly.
import { PreparedSignOfd as PreparedSignOfClass } from '../index';
