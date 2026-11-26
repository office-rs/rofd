// Signature status (spec 2026-11-23 §9, M12, M13): what the status bar
// says about the signatures of the document, and the detail view's texts.
// Pure functions over the SDK's reports, so they run in tests without the
// wasm module. Which signatures a save would invalidate is the SDK's
// `newlyInvalidated`.

import type {
  IntegrityResult,
  SignatureOverall,
  SignatureReport,
  SignatureValue,
} from '@office-rs/rofd';

/** The status bar's one line about the signatures: `null` (the entry is
 * not shown) when the document has none. A failure outranks everything
 * else; "仅未验证" counts the signatures that could not be confirmed, the
 * others being valid. */
export function summaryText(reports: SignatureReport[]): string | null {
  if (reports.length === 0) {
    return null;
  }
  const count = (overall: string): number =>
    reports.filter((report) => report.overall === overall).length;
  const invalid = count('Invalid');
  if (invalid > 0) {
    return `签章：有失效（${invalid}/${reports.length}）`;
  }
  const valid = count('Valid');
  if (valid === reports.length) {
    return `签章：全部有效（${valid}）`;
  }
  return `签章：仅未验证（${reports.length - valid}）`;
}

// ---- the detail view's texts (spec §13) ----

/** The verdict on one signature, for the detail view. */
export function verdictText(overall: SignatureOverall): string {
  switch (overall) {
    case 'Valid':
      return '有效';
    case 'Invalid':
      return '失效';
    case 'Unverified':
      return '未验证';
    case 'Unparseable':
      return '无法解析';
    case 'Unsupported':
      return '不支持';
  }
}

// ---- what a save has to say (spec M14, M20) ----

/** What else a successful save has to tell the user: nothing in the normal
 * case. An exhausted ID reserve is written all the same, and so is a
 * fallback to the full write; neither may pass silently. */
export function saveNotes(flags: {
  reserveExhausted: boolean;
  fullWriteFallback: boolean;
}): string[] {
  const notes: string[] = [];
  if (flags.reserveExhausted) {
    notes.push(
      '签章时预留的批注编号已用完：新批注的编号超出了文件记录的最大编号，其他软件之后新建的对象可能与之重复',
    );
  }
  if (flags.fullWriteFallback) {
    notes.push('这份文件无法只改写批注，已整份重新保存（文件没有签章，不影响验签）');
  }
  return notes;
}

/** Layer 1: do the protected files still hash to the recorded values. The
 * files that changed are listed below the line; a signature whose
 * reference list is empty protects nothing, which is a failure of its own. */
export function integrityText(integrity: IntegrityResult): string {
  if (integrity.ok) {
    return '有效';
  }
  return integrity.protected_files === 0 ? '失效（该签名未保护任何文件）' : '失效';
}

/** Layer 2: does the standalone seal equal the one inside the signature
 * value. `null` (nothing to compare) shows as a dash. */
export function sealMatchText(matches: boolean | null): string {
  if (matches === null) {
    return '—';
  }
  return matches ? '一致' : '不一致';
}

/** Layer 3: does the signature value verify, with its reason when not. */
export function signatureValueText(value: SignatureValue): string {
  const text = (() => {
    switch (value.status) {
      case 'Ok':
        return '有效';
      case 'Fail':
        return '失败';
      case 'Unverifiable':
        return '无法验证';
    }
  })();
  return value.reason === null || value.reason === ''
    ? text
    : `${text}（${value.reason}）`;
}

/** The first eight hex digits of a digest the report carries as base64:
 * enough to tell the recorded one from the computed one. Empty for a
 * missing file, or text that is not base64. */
export function digestHead(base64: string): string {
  if (base64 === '') {
    return '';
  }
  let binary: string;
  try {
    binary = atob(base64);
  } catch {
    return '';
  }
  let hex = '';
  for (let i = 0; i < binary.length && hex.length < 8; i += 1) {
    hex += binary.charCodeAt(i).toString(16).padStart(2, '0');
  }
  return hex.slice(0, 8);
}

/** A certificate's time (`YYYYMMDDHHMMSSZ`, as the report carries it) as
 * a day. */
function day(time: string): string {
  return time.length >= 8 ? `${time.slice(0, 4)}-${time.slice(4, 6)}-${time.slice(6, 8)}` : time;
}

/** The days a certificate is good for; a dash when neither is known. */
export function validityText(notBefore: string | null, notAfter: string | null): string {
  if (notBefore === null && notAfter === null) {
    return '—';
  }
  if (notAfter === null) {
    return `${day(notBefore ?? '')} 起`;
  }
  if (notBefore === null) {
    return `至 ${day(notAfter)}`;
  }
  return `${day(notBefore)} 至 ${day(notAfter)}`;
}
