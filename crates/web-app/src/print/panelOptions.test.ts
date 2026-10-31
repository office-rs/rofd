import { describe, expect, it } from 'vitest';
import { buildPrintOptions } from './panelOptions';

describe('buildPrintOptions', () => {
  it('maps panel checkboxes to PrintOptions', () => {
    expect(buildPrintOptions(false, false, true)).toEqual({ watermark: false, annotations: false, seal: true });
    expect(buildPrintOptions(true, true, false)).toEqual({ watermark: true, annotations: true, seal: false });
  });
});
