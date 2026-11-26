// Sign dialog decisions (spec 2026-11-23 §13): the pure brain of the web
// host's signing dialog, kept outside Vue so it runs in tests without
// Vue, antd, or the SDK. The dialog component renders a `SignDialogState`
// and forwards user edits as patches; these functions decide whether the
// dialog can be submitted and what the SDK is asked to do.
//
// `signsDirectly` mirrors rofd-sign's `SignRequest::signs_directly`: a
// signature without a picture, or an auto-stamped seal, is made right
// away; anything else waits for the click that says where the seal goes.

import type { CertificateEntry, SignOptions, SignPlacement } from '@office-rs/rofd';

/** What the host does for the dialog around a certificate file. */
export interface SignDialogServices {
  /** Import a certificate file: open it once with `password` to read what
   * to show for it, keep it in the store, and resolve to its entry
   * (rejects on a wrong password; nothing is stored then). */
  importCertificate(file: Uint8Array, password: string): Promise<CertificateEntry>;
  /** Check the password of a listed certificate; rejects when it is wrong. */
  checkPassword(certificateId: string, password: string): Promise<void>;
}

/** A validated dialog the host is to act on (the password was checked). */
export interface SignDialogRequest {
  certificateId: string;
  password: string;
  seal: SealChoice;
  autoPlace: boolean;
  rightMm: number;
  bottomMm: number;
  lastPageOnly: boolean;
}

/** The long side of a seal on the page, millimetres. */
export const SEAL_SIZE_MM = 40;

/** Which seal picture the dialog offers: none (an electronic signature),
 * one of the built-in pictures, or a file the user picked. */
export type SealChoice =
  | { kind: 'none' }
  | { kind: 'builtin'; index: number }
  | { kind: 'file'; name: string };

export interface SignDialogState {
  certificateId: string | null;
  /** The certificate's password; checked before anything is signed. */
  password: string;
  seal: SealChoice;
  /** 自动盖章: stamp at the given position instead of waiting for a click. */
  autoPlace: boolean;
  /** Distance from the right edge of the page, millimetres. */
  rightMm: number;
  /** Distance from the bottom edge of the page, millimetres. */
  bottomMm: number;
  /** 最后一页: the last page only; otherwise every page. */
  lastPageOnly: boolean;
  busy: boolean;
  error: string | null;
}

/** The defaults of the dialog (spec §13): the first built-in seal, placed
 * by hand. An electronic signature without a picture is something the user
 * chooses; nothing is signed until a certificate and its password are
 * given. */
export function emptyDialogState(): SignDialogState {
  return {
    certificateId: null,
    password: '',
    seal: { kind: 'builtin', index: 0 },
    autoPlace: false,
    rightMm: 10,
    bottomMm: 10,
    lastPageOnly: true,
    busy: false,
    error: null,
  };
}

function isPositive(value: number): boolean {
  return Number.isFinite(value) && value > 0;
}

/** Why the dialog cannot be submitted yet; `null` when it can. */
export function validateDialog(state: SignDialogState): string | null {
  if (state.busy) {
    return '正在签章，请稍候';
  }
  if (state.certificateId === null) {
    return '请选择证书';
  }
  if (state.password === '') {
    return '请输入证书口令';
  }
  if (state.seal.kind === 'file' && state.seal.name === '') {
    return '请选择章图';
  }
  if (state.autoPlace && !(isPositive(state.rightMm) && isPositive(state.bottomMm))) {
    return '距右边和距底部必须是大于 0 的数';
  }
  return null;
}

/** Whether the document can be signed right away, without waiting for the
 * click that places the seal: there is no picture, or the position is
 * given. */
export function signsDirectly(state: PlacementFields): boolean {
  return state.seal.kind === 'none' || state.autoPlace;
}

/** The placement fields the option builders read: the dialog's state, or a
 * request made from it. */
export type PlacementFields = Pick<
  SignDialogState,
  'seal' | 'autoPlace' | 'rightMm' | 'bottomMm' | 'lastPageOnly'
>;

/** The position of an auto-stamped seal; `null` when the user places it. */
export function autoPlacement(state: PlacementFields): SignPlacement | null {
  if (!state.autoPlace) {
    return null;
  }
  return {
    bottomRight: {
      pages: state.lastPageOnly ? 'last' : 'all',
      rightMm: state.rightMm,
      bottomMm: state.bottomMm,
    },
  };
}

/** The SDK's options for a signature made right away: an electronic
 * signature without a picture, or an auto-stamped seal (whose picture
 * `png` is). */
export function directSignOptions(
  state: PlacementFields,
  png: Uint8Array | null,
  now: string,
): SignOptions {
  return {
    ...(state.seal.kind === 'none' || png === null
      ? {}
      : { seal: { png, sizeMm: SEAL_SIZE_MM } }),
    ...(state.autoPlace ? { placement: autoPlacement(state) } : {}),
    now,
  };
}

/** The SDK's options for a seal the user placed by clicking: page numbers
 * start at 1 in the SDK's `at` placement. */
export function placedSignOptions(
  state: PlacementFields,
  png: Uint8Array,
  pageIndex: number,
  boundary: { x: number; y: number; w: number; h: number },
  now: string,
): SignOptions {
  return {
    seal: { png, sizeMm: SEAL_SIZE_MM },
    placement: { at: { page: pageIndex + 1, boundary } },
    now,
  };
}

/** `now` as the SDK takes it: `YYYY-MM-DD HH:mm:ss`, local time. The
 * library never reads the clock; the host does (spec §13). */
export function formatNow(date: Date): string {
  const pad = (n: number): string => String(n).padStart(2, '0');
  return (
    `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())}` +
    ` ${pad(date.getHours())}:${pad(date.getMinutes())}:${pad(date.getSeconds())}`
  );
}

/** A seal picture's size on the page: the long side is
 * [`SEAL_SIZE_MM`], the other side follows the pixel ratio (the rule
 * rofd-sign applies when it signs). */
export function sealSizeMm(
  widthPx: number,
  heightPx: number,
): { widthMm: number; heightMm: number } {
  const scale = SEAL_SIZE_MM / Math.max(widthPx, heightPx, 1);
  return { widthMm: widthPx * scale, heightMm: heightPx * scale };
}
