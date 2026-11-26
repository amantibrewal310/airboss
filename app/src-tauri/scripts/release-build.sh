#!/bin/sh
# Builds a release: the .dmg for new installs and the signed update for installed apps
# (SPEC-updates), then checks them. Run from anywhere: sh app/src-tauri/scripts/release-build.sh
#
# The updater's private key is ~/.tauri/airboss-updater.key (or $AIRBOSS_UPDATER_KEY) and its
# password the login keychain item "airboss-updater-key". Neither is ever in the repo.
#
# Leaves in target/release/bundle/release/ the four files a GitHub release carries:
#   Airboss_<version>_aarch64.dmg, its .sha256, and Airboss_<version>_aarch64.app.tar.gz(.sig)

set -eu
set +x # never trace: the key's password is in this script's environment

root=$(cd "$(dirname "$0")/../../.." && pwd)
fail() { echo "release-build: $*" >&2; exit 1; }
# tauri-build merges $TAURI_CONFIG into the compiled config: a QA config left in the shell
# (a loopback endpoint, plain HTTP) must never reach a release.
unset TAURI_CONFIG
endpoint=https://raw.githubusercontent.com/amantibrewal310/airboss/main/updates/latest.json

key=${AIRBOSS_UPDATER_KEY:-$HOME/.tauri/airboss-updater.key}
[ -f "$key" ] || fail "no updater key at $key"
password=$(security find-generic-password -s airboss-updater-key -w 2>/dev/null) ||
  fail "no keychain item airboss-updater-key with the key's password"

version=$(cd "$root/app" && node -p "require('./package.json').version")
echo "Building Airboss $version"

# Tauri signs the update archive when it sees these. TAURI_SIGNING_PRIVATE_KEY takes the key's
# path (tauri build ignores TAURI_SIGNING_PRIVATE_KEY_PATH), so the key's bytes aren't in the
# environment of every build script; the password has to be.
TAURI_SIGNING_PRIVATE_KEY=$key
TAURI_SIGNING_PRIVATE_KEY_PASSWORD=$password
export TAURI_SIGNING_PRIVATE_KEY TAURI_SIGNING_PRIVATE_KEY_PASSWORD
(cd "$root/app" && CI=true pnpm bundle --bundles app,dmg)
unset TAURI_SIGNING_PRIVATE_KEY TAURI_SIGNING_PRIVATE_KEY_PASSWORD password

bundle=$root/target/release/bundle
app=$bundle/macos/Airboss.app
codesign --verify --deep --strict "$app" || fail "the app's signature doesn't verify"
short=$(/usr/libexec/PlistBuddy -c "Print :CFBundleShortVersionString" "$app/Contents/Info.plist")
build=$(/usr/libexec/PlistBuddy -c "Print :CFBundleVersion" "$app/Contents/Info.plist")
cli=$("$app/Contents/MacOS/airboss" --version)
echo "Info.plist: $short ($build); $cli"
[ "$cli" = "airboss $version" ] || fail "the bundled CLI says '$cli', not $version"

# The compiled-in updater config is the production one.
binary=$app/Contents/MacOS/airboss-app
strings "$binary" | grep -qF "$endpoint" || fail "the app doesn't have the production update endpoint"
# Every build embeds `build.devUrl` (the Vite dev server); any other loopback URL means a QA
# config leaked in.
devurl=$(cd "$root/app" && node -p "require('./src-tauri/tauri.conf.json').build.devUrl")
loopback=$(strings "$binary" | grep -oE 'https?://(127\.0\.0\.1|localhost)(:[0-9]+)?' | sort -u | grep -vxF "$devurl" || true)
[ -z "$loopback" ] || fail "the app has a loopback URL compiled in ($loopback): a QA config leaked into the build"

archive=$bundle/macos/Airboss.app.tar.gz
[ -s "$archive" ] && [ -s "$archive.sig" ] || fail "no signed update archive: is createUpdaterArtifacts on?"
# The signature verifies with the app's public key and is signed as this version
# (requireSignedVersion: installed apps check the same).
(cd "$root" && node --input-type=module -e "
  import { readFileSync } from 'node:fs';
  import { verifyUpdateSignature } from './app/src-tauri/scripts/update-manifest.mjs';
  const conf = JSON.parse(readFileSync('app/src-tauri/tauri.conf.json', 'utf8'));
  const signed = verifyUpdateSignature({
    pubkeyB64: conf.plugins.updater.pubkey,
    sigB64: readFileSync(process.argv[1], 'utf8'),
    data: readFileSync(process.argv[2]),
  });
  if (signed.version !== process.argv[3]) throw new Error('signed as ' + signed.version + ', not ' + process.argv[3]);
  console.log('Update signature: verified, version ' + signed.version);
" "$archive.sig" "$archive" "$version") || fail "the update archive's signature doesn't check out"

out=$bundle/release
rm -rf "$out"
mkdir -p "$out"
dmg=Airboss_${version}_aarch64.dmg
cp "$bundle/dmg/$dmg" "$out/$dmg"
(cd "$out" && shasum -a 256 "$dmg" > "$dmg.sha256")
cp "$archive" "$out/Airboss_${version}_aarch64.app.tar.gz"
cp "$archive.sig" "$out/Airboss_${version}_aarch64.app.tar.gz.sig"

echo
echo "Ready in $out:"
ls -l "$out"
echo
echo "Next: tag v$version, publish the GitHub pre-release with those four files, then"
echo "  node app/src-tauri/scripts/update-manifest.mjs $version"
echo "and commit updates/latest.json to main."
