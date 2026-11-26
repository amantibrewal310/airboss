#!/usr/bin/env node
// Writes `updates/latest.json`, the manifest installed copies of Airboss check for a new
// version (SPEC-updates). Run it after the GitHub release is published with its assets:
//
//   node app/src-tauri/scripts/update-manifest.mjs <version>
//
// The notes are the version's CHANGELOG.md section. The archive and its `.sig` come from the
// published GitHub release, not from `target/` (which the next build overwrites): the script
// downloads both, verifies the signature against the app's public key in tauri.conf.json, and
// checks that the signed version is this version. So the manifest never names a file that
// isn't there, or pairs it with the wrong signature.

import { createHash, createPublicKey, verify } from 'node:crypto';
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

const ED25519_SPKI = Buffer.from('302a300506032b6570032100', 'hex');

/** The two lines of a base64-wrapped minisign file (a `.sig` or Tauri's `pubkey`). */
function minisignLines(b64) {
  return Buffer.from(b64.trim(), 'base64').toString('utf8').split('\n').filter((l) => l !== '');
}

/**
 * Verifies a Tauri update signature (minisign) over `data` with `pubkeyB64`, both as Tauri
 * writes them (base64 of the minisign text). Returns the signed trusted comment's fields, e.g.
 * `{ version, file, timestamp }`; throws when anything doesn't match.
 */
export function verifyUpdateSignature({ pubkeyB64, sigB64, data }) {
  const [, keyLine] = minisignLines(pubkeyB64);
  const key = Buffer.from(keyLine ?? '', 'base64');
  if (key.length !== 42 || key.subarray(0, 2).toString() !== 'Ed') throw new Error('not a minisign Ed25519 public key');
  const publicKey = createPublicKey({ key: Buffer.concat([ED25519_SPKI, key.subarray(10)]), format: 'der', type: 'spki' });

  const [, sigLine, trustedLine, globalLine] = minisignLines(sigB64);
  const sig = Buffer.from(sigLine ?? '', 'base64');
  if (sig.length !== 74) throw new Error('not a minisign signature');
  const alg = sig.subarray(0, 2).toString();
  if (alg !== 'ED' && alg !== 'Ed') throw new Error(`unknown signature algorithm ${alg}`);
  if (!sig.subarray(2, 10).equals(key.subarray(2, 10))) throw new Error("the signature's key id isn't the app's key");
  const message = alg === 'ED' ? createHash('blake2b512').update(data).digest() : data;
  if (!verify(null, message, publicKey, sig.subarray(10))) throw new Error("the signature doesn't match the archive");

  const prefix = 'trusted comment: ';
  if (!trustedLine?.startsWith(prefix)) throw new Error('the signature has no trusted comment');
  const trusted = trustedLine.slice(prefix.length);
  const global = Buffer.from(globalLine ?? '', 'base64');
  if (!verify(null, Buffer.concat([sig.subarray(10), Buffer.from(trusted)]), publicKey, global)) {
    throw new Error("the signature's trusted comment was changed");
  }
  return Object.fromEntries(
    trusted.split('\t').map((kv) => {
      const at = kv.indexOf(':');
      return [kv.slice(0, at), kv.slice(at + 1)];
    }),
  );
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
  const conf = JSON.parse(readFileSync(join(root, 'app/src-tauri/tauri.conf.json'), 'utf8'));
  const download = async (url) => {
    const res = await fetch(url, { redirect: 'follow' });
    if (!res.ok) throw new Error(`${url} isn't downloadable (${res.status}): publish the release with it first`);
    return Buffer.from(await res.arrayBuffer());
  };
  const url = assetUrl(version);
  const [archive, sig] = await Promise.all([download(url), download(`${url}.sig`)]);
  const signature = sig.toString('utf8').trim();
  const signed = verifyUpdateSignature({ pubkeyB64: conf.plugins.updater.pubkey, sigB64: signature, data: archive });
  if (signed.version !== version) {
    throw new Error(`the archive is signed as version ${signed.version ?? '(none)'}, not ${version}`);
  }
  console.log(`verified ${assetName(version)}: ${archive.length} bytes, signed as ${signed.version}`);
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
