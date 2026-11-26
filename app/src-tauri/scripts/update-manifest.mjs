#!/usr/bin/env node
// Writes `updates/latest.json`, the manifest installed copies of Airboss check for a new
// version (SPEC-updates). Run it after the GitHub release is published with its assets:
//
//   node app/src-tauri/scripts/update-manifest.mjs <version>
//
// The notes are the version's CHANGELOG.md section, and the signature is the `.sig` the
// release build wrote next to `Airboss.app.tar.gz`. Before writing, it checks that the release's
// update archive downloads, so the manifest never names a file that isn't there.

import { readFileSync, writeFileSync, mkdirSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

export const REPO = 'https://github.com/amantibrewal310/airboss';

const SEMVER = /^\d+\.\d+\.\d+(-[0-9A-Za-z-]+(\.[0-9A-Za-z-]+)*)?$/;

/** The update archive's name on the GitHub release. */
export function assetName(version) {
  return `Airboss_${version}_aarch64.app.tar.gz`;
}

/** Where installed apps download `version` from. */
export function assetUrl(version) {
  return `${REPO}/releases/download/v${version}/${assetName(version)}`;
}

/** `version`'s section of CHANGELOG.md, without its heading; throws when there isn't one. */
export function changelogSection(changelog, version) {
  const lines = changelog.split('\n');
  const start = lines.findIndex((l) => l.startsWith(`## [${version}] - `));
  if (start === -1) throw new Error(`CHANGELOG.md has no section for ${version}`);
  const rest = lines.slice(start + 1);
  const end = rest.findIndex((l) => l.startsWith('## [') || /^\[[^\]]+\]: /.test(l));
  const body = (end === -1 ? rest : rest.slice(0, end)).join('\n').trim();
  if (!body) throw new Error(`CHANGELOG.md's section for ${version} is empty`);
  return body;
}

/** The manifest, in the shape Tauri's updater reads (its "static JSON"). */
export function manifest({ version, notes, signature, pubDate }) {
  if (!SEMVER.test(version)) throw new Error(`not a version: ${version}`);
  const sig = signature.trim();
  if (!sig || /\s/.test(sig)) throw new Error('the signature should be one base64 line');
  if (Number.isNaN(Date.parse(pubDate))) throw new Error(`not a date: ${pubDate}`);
  return {
    version,
    notes,
    pub_date: pubDate,
    platforms: {
      'darwin-aarch64': { signature: sig, url: assetUrl(version) },
    },
  };
}

async function main(argv) {
  const version = argv[0];
  if (!version) throw new Error('usage: update-manifest.mjs <version>');
  const root = join(dirname(fileURLToPath(import.meta.url)), '../../..');
  const notes = changelogSection(readFileSync(join(root, 'CHANGELOG.md'), 'utf8'), version);
  const signature = readFileSync(
    join(root, 'target/release/bundle/macos/Airboss.app.tar.gz.sig'),
    'utf8',
  );
  const url = assetUrl(version);
  const res = await fetch(url, { method: 'HEAD', redirect: 'follow' });
  if (!res.ok) throw new Error(`${url} isn't downloadable yet (${res.status}): publish the release first`);
  const out = manifest({ version, notes, signature, pubDate: new Date().toISOString() });
  mkdirSync(join(root, 'updates'), { recursive: true });
  writeFileSync(join(root, 'updates/latest.json'), `${JSON.stringify(out, null, 2)}\n`);
  console.log(`wrote updates/latest.json for ${version}; commit and push it to main`);
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  main(process.argv.slice(2)).catch((e) => {
    console.error(`update-manifest: ${e.message}`);
    process.exit(1);
  });
}
