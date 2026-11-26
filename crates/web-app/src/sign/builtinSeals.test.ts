import { describe, expect, it } from 'vitest';
import { BUILTIN_SEALS } from './builtinSeals';

describe('BUILTIN_SEALS', () => {
  it('lists three pictures with distinct urls', () => {
    expect(BUILTIN_SEALS.map((seal) => seal.name)).toEqual(['圆章', '方章', '条章']);
    const urls = BUILTIN_SEALS.map((seal) => seal.url);
    expect(new Set(urls).size).toBe(3);
    for (const url of urls) {
      expect(url).toMatch(/\/seals\/[a-z]+\.png$/);
    }
  });
});
