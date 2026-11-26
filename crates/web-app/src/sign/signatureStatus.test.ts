import { describe, expect, it } from 'vitest';
import type { SignatureOverall, SignatureReport } from '@office-rs/rofd';
import {
  digestHead,
  integrityText,
  saveNotes,
  sealMatchText,
  signatureValueText,
  summaryText,
  validityText,
  verdictText,
} from './signatureStatus';

function report(id: string, overall: SignatureOverall): SignatureReport {
  return {
    id,
    kind: 'Seal',
    variant: 'SesV4',
    overall,
    integrity: { ok: true, failures: [], protected_files: 1 },
    seal_match: null,
    signature_value: { status: 'Ok', reason: null },
    certificate: null,
    provider: null,
    date_time: null,
  };
}

describe('summaryText', () => {
  it('names each summary and is null without signatures', () => {
    expect(summaryText([])).toBeNull();
    const valid = report('1', 'Valid');
    expect(summaryText([valid, report('2', 'Valid')])).toBe('签章：全部有效（2）');
    // A failure outranks everything else.
    expect(summaryText([valid, report('2', 'Invalid'), report('3', 'Valid')])).toBe(
      '签章：有失效（1/3）',
    );
    expect(
      summaryText([report('1', 'Unverified'), report('2', 'Unparseable')]),
    ).toBe('签章：仅未验证（2）');
    expect(
      summaryText([report('1', 'Unsupported'), report('2', 'Invalid')]),
    ).toBe('签章：有失效（1/2）');
    // Nothing failed and not everything could be confirmed: the count is of
    // those that could not.
    expect(summaryText([valid, report('2', 'Unverified')])).toBe(
      '签章：仅未验证（1）',
    );
  });
});

describe('detail texts', () => {
  it('names each verdict, seal check and signature value', () => {
    expect(verdictText('Valid')).toBe('有效');
    expect(verdictText('Invalid')).toBe('失效');
    expect(verdictText('Unverified')).toBe('未验证');
    expect(verdictText('Unparseable')).toBe('无法解析');
    expect(verdictText('Unsupported')).toBe('不支持');
    expect(sealMatchText(null)).toBe('—');
    expect(sealMatchText(true)).toBe('一致');
    expect(sealMatchText(false)).toBe('不一致');
    expect(signatureValueText({ status: 'Ok', reason: null })).toBe('有效');
    expect(signatureValueText({ status: 'Unverifiable', reason: null })).toBe(
      '无法验证',
    );
    expect(
      signatureValueText({ status: 'Fail', reason: 'the data hash differs' }),
    ).toBe('失败（the data hash differs）');
  });

  it('digestHead shows the first eight hex digits of a digest', () => {
    // base64 of 01 23 45 67 89 ab cd ef.
    expect(digestHead('ASNFZ4mrze8=')).toBe('01234567');
    expect(digestHead('')).toBe('');
    expect(digestHead('not base64!')).toBe('');
  });

  it('validityText names the days a certificate is good for', () => {
    expect(validityText('20200101000000Z', '20991231235959Z')).toBe(
      '2020-01-01 至 2099-12-31',
    );
    expect(validityText(null, null)).toBe('—');
    expect(validityText('20200101000000Z', null)).toBe('2020-01-01 起');
    expect(validityText(null, '20991231235959Z')).toBe('至 2099-12-31');
  });
});

describe('integrityText', () => {
  it('says when a signature protects no file at all', () => {
    expect(integrityText({ ok: true, failures: [], protected_files: 3 })).toBe('有效');
    // Nothing failed because nothing was checked: that is not a pass.
    expect(integrityText({ ok: false, failures: [], protected_files: 0 })).toBe(
      '失效（该签名未保护任何文件）',
    );
    expect(
      integrityText({
        ok: false,
        failures: [
          { file_ref: '/Doc_0/Pages/Page_0/Content.xml', expected: '', actual: '' },
        ],
        protected_files: 2,
      }),
    ).toBe('失效');
  });
});

describe('saveNotes', () => {
  it('is silent for a normal save and names each flag', () => {
    // Spec M14 and M20: the library reports both, and the user hears of
    // them here (final review I5).
    expect(saveNotes({ reserveExhausted: false, fullWriteFallback: false })).toEqual([]);
    const reserve = saveNotes({ reserveExhausted: true, fullWriteFallback: false });
    expect(reserve).toHaveLength(1);
    expect(reserve[0]).toContain('预留');
    const fallback = saveNotes({ reserveExhausted: false, fullWriteFallback: true });
    expect(fallback).toHaveLength(1);
    expect(fallback[0]).toContain('整份');
    expect(saveNotes({ reserveExhausted: true, fullWriteFallback: true })).toHaveLength(2);
  });
});
