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

root=$(cd "$(dirname "$0")/../../.." && pwd)
fail() { echo "release-build: $*" >&2; exit 1; }

key=${AIRBOSS_UPDATER_KEY:-$HOME/.tauri/airboss-updater.key}
[ -f "$key" ] || fail "no updater key at $key"
password=$(security find-generic-password -s airboss-updater-key -w 2>/dev/null) ||
  fail "no keychain item airboss-updater-key with the key's password"

version=$(cd "$root/app" && node -p "require('./package.json').version")
echo "Building Airboss $version"

# Tauri signs the update archive when it sees these.
TAURI_SIGNING_PRIVATE_KEY=$(cat "$key")
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

archive=$bundle/macos/Airboss.app.tar.gz
[ -s "$archive" ] && [ -s "$archive.sig" ] || fail "no signed update archive: is createUpdaterArtifacts on?"

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
