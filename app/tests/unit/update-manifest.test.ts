import { describe, expect, it } from 'vitest';
// @ts-expect-error: a plain .mjs release script, without type declarations.
import { assetUrl, changelogSection, manifest } from '../../src-tauri/scripts/update-manifest.mjs';

const CHANGELOG = `# Changelog

## [Unreleased]

## [0.1.0-alpha.4] - 2026-11-27

### Added

- Updates inside the app.

## [0.1.0-alpha.3] - 2026-11-26

### Fixed

- A fix.

[Unreleased]: https://github.com/amantibrewal310/airboss/compare/v0.1.0-alpha.4...HEAD
[0.1.0-alpha.4]: https://github.com/amantibrewal310/airboss/compare/v0.1.0-alpha.3...v0.1.0-alpha.4
`;

describe('the update manifest (SPEC-updates)', () => {
  it('points at the release asset for the version', () => {
    expect(assetUrl('0.1.0-alpha.4')).toBe(
      'https://github.com/amantibrewal310/airboss/releases/download/v0.1.0-alpha.4/Airboss_0.1.0-alpha.4_aarch64.app.tar.gz',
    );
  });

  it('takes the notes from the version’s changelog section only', () => {
    expect(changelogSection(CHANGELOG, '0.1.0-alpha.4')).toBe('### Added\n\n- Updates inside the app.');
    expect(changelogSection(CHANGELOG, '0.1.0-alpha.3')).toBe('### Fixed\n\n- A fix.');
    expect(() => changelogSection(CHANGELOG, '0.1.0-alpha.9')).toThrow('no section for 0.1.0-alpha.9');
  });

  it('is the static JSON Tauri’s updater reads', () => {
    const m = manifest({ version: '0.1.0-alpha.4', notes: 'n', signature: 'c2ln\n', pubDate: '2026-11-27T10:00:00.000Z' });
    expect(m).toEqual({
      version: '0.1.0-alpha.4',
      notes: 'n',
      pub_date: '2026-11-27T10:00:00.000Z',
      platforms: {
        'darwin-aarch64': {
          signature: 'c2ln',
          url: 'https://github.com/amantibrewal310/airboss/releases/download/v0.1.0-alpha.4/Airboss_0.1.0-alpha.4_aarch64.app.tar.gz',
        },
      },
    });
  });

  it('refuses a bad version, signature or date', () => {
    const ok = { version: '0.1.0', notes: '', signature: 'c2ln', pubDate: '2026-11-27T10:00:00Z' };
    expect(() => manifest({ ...ok, version: 'v0.1.0' })).toThrow('not a version');
    expect(() => manifest({ ...ok, signature: '' })).toThrow('one base64 line');
    expect(() => manifest({ ...ok, signature: 'a b' })).toThrow('one base64 line');
    expect(() => manifest({ ...ok, pubDate: 'soon' })).toThrow('not a date');
  });
});
