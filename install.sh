#!/bin/sh
# Installs (or updates) Airboss from its newest GitHub release, alphas included.
#
#   curl -fsSL https://raw.githubusercontent.com/amantibrewal310/airboss/main/install.sh | sh
#
# It downloads the .dmg and its SHA-256, checks one against the other, copies Airboss.app to
# /Applications and opens it. A download made with curl isn't flagged by macOS, so the
# "Apple could not verify" warning for an app that isn't notarized doesn't come up.
#
# AIRBOSS_TAG=v0.1.0-alpha.1 installs that release instead of the newest one.
# AIRBOSS_APPS=<folder> installs somewhere other than /Applications, and doesn't open it.
set -eu

REPO=amantibrewal310/airboss
APPS=${AIRBOSS_APPS:-/Applications}

say() { printf '%s\n' "$*"; }
fail() { printf 'airboss install: %s\n' "$*" >&2; exit 1; }

[ "$(uname -s)" = Darwin ] || fail "Airboss is a Mac app."
[ "$(uname -m)" = arm64 ] || fail "this build needs a Mac with Apple silicon."
major=$(sw_vers -productVersion | cut -d. -f1)
[ "$major" -ge 13 ] || fail "Airboss needs macOS 13 or later."

tmp=$(mktemp -d)
mnt="$tmp/mnt"
cleanup() {
  hdiutil detach -quiet "$mnt" 2>/dev/null || true
  rm -rf "$tmp"
}
trap cleanup EXIT INT TERM

tag=${AIRBOSS_TAG:-}
if [ -z "$tag" ]; then
  # The newest release, pre-releases included; plutil reads JSON.
  curl -fsSL -H 'Accept: application/vnd.github+json' \
    "https://api.github.com/repos/$REPO/releases?per_page=1" -o "$tmp/releases.json" ||
    fail "couldn't reach GitHub to find the newest release."
  tag=$(plutil -extract 0.tag_name raw -o - "$tmp/releases.json" 2>/dev/null) ||
    fail "couldn't find a release of $REPO."
fi
version=${tag#v}
dmg="Airboss_${version}_aarch64.dmg"
base="https://github.com/$REPO/releases/download/$tag"

say "Downloading Airboss ${version}..."
curl -fL --progress-bar "$base/$dmg" -o "$tmp/$dmg" || fail "couldn't download $dmg."
curl -fsSL "$base/$dmg.sha256" -o "$tmp/$dmg.sha256" || fail "couldn't download $dmg.sha256."
(cd "$tmp" && shasum -a 256 -c "$dmg.sha256" >/dev/null) ||
  fail "the download doesn't match its SHA-256. Nothing was installed."

if [ -z "${AIRBOSS_APPS:-}" ] && pgrep -xq airboss-app; then
  fail "Airboss is running. Quit it (⌘Q, or Quit Airboss in its menu-bar menu), then run this again."
fi

mkdir -p "$mnt"
hdiutil attach -quiet -nobrowse -readonly -mountpoint "$mnt" "$tmp/$dmg" || fail "couldn't open $dmg."
codesign --verify --deep --strict "$mnt/Airboss.app" 2>/dev/null ||
  fail "the app's signature doesn't check out. Nothing was installed."

mkdir -p "$APPS"
rm -rf "$APPS/Airboss.app"
ditto "$mnt/Airboss.app" "$APPS/Airboss.app" || fail "couldn't copy Airboss to $APPS."
# Belt and braces: nothing here was downloaded by a browser, but clear the flag anyway.
xattr -dr com.apple.quarantine "$APPS/Airboss.app" 2>/dev/null || true

say "Installed Airboss $version in $APPS."
if [ -z "${AIRBOSS_APPS:-}" ]; then
  open "$APPS/Airboss.app"
  say "It's open: the first-run screen walks you through the rest."
fi
