import { describe, expect, it } from 'vitest';
import {
  autoPlacement,
  directSignOptions,
  emptyDialogState,
  formatNow,
  placedSignOptions,
  sealSizeMm,
  signsDirectly,
  validateDialog,
  type SignDialogState,
} from './signDialogState';

describe('sign dialog state', () => {
  it('emptyDialogState has the defaults of the dialog', () => {
    // The first built-in seal: an electronic signature without a picture
    // is something the user chooses (spec §13).
    expect(emptyDialogState()).toEqual({
      certificateId: null,
      password: '',
      seal: { kind: 'builtin', index: 0 },
      autoPlace: false,
      rightMm: 10,
      bottomMm: 10,
      lastPageOnly: true,
      busy: false,
      error: null,
    });
  });

  it('validateDialog requires a certificate and rejects margins that are not positive', () => {
    expect(validateDialog(emptyDialogState())).toBe('请选择证书');
    expect(
      validateDialog({ ...emptyDialogState(), certificateId: 'a', password: '' }),
    ).toBe('请输入证书口令');

    const ready: SignDialogState = {
      ...emptyDialogState(),
      certificateId: 'a',
      password: 'secret',
    };
    expect(validateDialog(ready)).toBeNull();
    expect(validateDialog({ ...ready, autoPlace: true })).toBeNull();

    const marginMessage = '距右边和距底部必须是大于 0 的数';
    for (const bad of [0, -1, Number.NaN, Number.POSITIVE_INFINITY]) {
      expect(
        validateDialog({ ...ready, autoPlace: true, rightMm: bad }),
        `rightMm ${bad}`,
      ).toBe(marginMessage);
      expect(
        validateDialog({ ...ready, autoPlace: true, bottomMm: bad }),
        `bottomMm ${bad}`,
      ).toBe(marginMessage);
    }
    // The margins are only read for auto-stamping.
    expect(
      validateDialog({ ...ready, autoPlace: false, rightMm: Number.NaN }),
    ).toBeNull();

    // A busy dialog is not submitted again.
    expect(validateDialog({ ...ready, busy: true })).toBe('正在签章，请稍候');
  });

  it('signsDirectly is true for no-picture and for auto-place', () => {
    const state: SignDialogState = {
      ...emptyDialogState(),
      certificateId: 'a',
      password: 'secret',
    };
    // A seal the user places with a click.
    expect(signsDirectly(state)).toBe(false);
    expect(signsDirectly({ ...state, seal: { kind: 'builtin', index: 2 } })).toBe(
      false,
    );
    // An electronic signature has no picture to place.
    expect(signsDirectly({ ...state, seal: { kind: 'none' } })).toBe(true);
    // Auto-stamping needs no click either, whatever the picture.
    expect(
      signsDirectly({
        ...state,
        seal: { kind: 'file', name: 'mine.png' },
        autoPlace: true,
      }),
    ).toBe(true);
    expect(
      signsDirectly({ ...state, seal: { kind: 'builtin', index: 2 }, autoPlace: true }),
    ).toBe(true);
  });
});

describe('sign options', () => {
  const ready: SignDialogState = {
    ...emptyDialogState(),
    certificateId: 'a',
    password: 'secret',
  };
  const png = new Uint8Array([137, 80]);

  it('directSignOptions follows the dialog', () => {
    // An electronic signature: no seal, no placement.
    expect(
      directSignOptions({ ...ready, seal: { kind: 'none' } }, null, '2026-11-24 10:30:00'),
    ).toEqual({ now: '2026-11-24 10:30:00' });
    // Auto-stamped on the last page.
    expect(
      directSignOptions(
        { ...ready, seal: { kind: 'builtin', index: 1 }, autoPlace: true },
        png,
        '2026-11-24 10:30:00',
      ),
    ).toEqual({
      seal: { png, sizeMm: 40 },
      placement: {
        bottomRight: { pages: 'last', rightMm: 10, bottomMm: 10 },
      },
      now: '2026-11-24 10:30:00',
    });
    // On every page, with the margins the user gave.
    expect(
      autoPlacement({ ...ready, autoPlace: true, lastPageOnly: false, rightMm: 15.5, bottomMm: 8 }),
    ).toEqual({
      bottomRight: { pages: 'all', rightMm: 15.5, bottomMm: 8 },
    });
    expect(autoPlacement(ready)).toBeNull();
  });

  it('placedSignOptions takes the page the SDK counts from 1', () => {
    expect(
      placedSignOptions(
        { ...ready, seal: { kind: 'file', name: 'mine.png' } },
        png,
        2,
        { x: 80, y: 80, w: 40, h: 20 },
        '2026-11-24 10:30:00',
      ),
    ).toEqual({
      seal: { png, sizeMm: 40 },
      placement: { at: { page: 3, boundary: { x: 80, y: 80, w: 40, h: 20 } } },
      now: '2026-11-24 10:30:00',
    });
  });

  it('formatNow writes local time as the SDK takes it', () => {
    expect(formatNow(new Date(2026, 10, 24, 9, 5, 3))).toBe('2026-11-24 09:05:03');
  });

  it('sealSizeMm puts the long side at 40 mm', () => {
    expect(sealSizeMm(256, 256)).toEqual({ widthMm: 40, heightMm: 40 });
    const bar = sealSizeMm(384, 128);
    expect(bar.widthMm).toBeCloseTo(40);
    expect(bar.heightMm).toBeCloseTo(40 / 3);
    const tall = sealSizeMm(100, 300);
    expect(tall.heightMm).toBeCloseTo(40);
    expect(tall.widthMm).toBeCloseTo(40 / 3);
  });
});
