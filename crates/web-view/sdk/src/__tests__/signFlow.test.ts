import { describe, expect, it } from 'vitest';
import type { SignatureOverall, SignatureReport } from '../index';
import { newlyInvalidated } from '../index';

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

describe('newlyInvalidated', () => {
  it('names the signatures that stopped being valid', () => {
    const before = [report('1', 'Valid'), report('2', 'Valid'), report('3', 'Invalid')];
    const after = [report('1', 'Valid'), report('2', 'Invalid'), report('3', 'Invalid')];
    expect(newlyInvalidated(before, after)).toEqual(['2']);
    // Unverified is not valid either.
    expect(
      newlyInvalidated(before, [
        report('1', 'Unverified'),
        report('2', 'Valid'),
        report('3', 'Invalid'),
      ]),
    ).toEqual(['1']);
    // Nothing changes, or a signature is added.
    expect(newlyInvalidated(before, before)).toEqual([]);
    expect(newlyInvalidated(before, [...before, report('4', 'Valid')])).toEqual([]);
    // A signature that vanished could not have stayed valid.
    expect(newlyInvalidated(before, [report('1', 'Valid')])).toEqual(['2']);
    // What was not valid before is not reported again.
    expect(newlyInvalidated([report('1', 'Invalid')], [])).toEqual([]);
    expect(newlyInvalidated([], [])).toEqual([]);
  });

  it('does not change the reports it is given', () => {
    const before = [report('1', 'Valid')];
    const after = [report('1', 'Invalid')];
    const kept = structuredClone(before);
    newlyInvalidated(before, after);
    expect(before).toEqual(kept);
  });
});
