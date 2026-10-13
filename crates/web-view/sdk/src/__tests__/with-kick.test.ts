import { describe, expect, it, vi } from 'vitest';
import { withKick } from '../with-kick';

describe('withKick', () => {
  it('kicks after method calls, forwards args/result/this', () => {
    const kick = vi.fn();
    const target = {
      value: 42,
      greet(name: string): string {
        return `hi ${name} ${this.value}`;
      },
    };
    const p = withKick(target, kick);
    expect(p.greet('a')).toBe('hi a 42');
    expect(kick).toHaveBeenCalledTimes(1);
  });

  it('exempts renderFrame and passes non-functions through', () => {
    const kick = vi.fn();
    const target = {
      renderFrame(): number {
        return -1;
      },
      value: 7,
    };
    const p = withKick(target, kick);
    expect(p.renderFrame()).toBe(-1);
    expect(p.value).toBe(7);
    expect(kick).not.toHaveBeenCalled();
  });

  it('memoizes wrappers (stable function identity)', () => {
    const p = withKick({ foo(): void {} }, () => {});
    expect(p.foo).toBe(p.foo);
  });
});
