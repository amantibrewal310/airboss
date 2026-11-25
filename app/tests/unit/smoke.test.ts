import { describe, expect, it } from 'vitest';

describe('toolchain', () => {
  it('runs unit tests', () => {
    expect(typeof globalThis.structuredClone).toBe('function');
  });
});
