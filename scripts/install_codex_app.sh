#!/usr/bin/env bash
set -euo pipefail

codex_app="/Applications/Codex.app"
codex_dmg_url="https://persistent.oaistatic.com/codex-app-prod/Codex.dmg"
expected_sha256="73d863ffeb8207ef566e6996033fdb1607272dd1d1760ae6dc0ec8ec9c623aea"

if [ -d "$codex_app" ]; then
  echo "Codex app already installed"
  exit 0
fi

tmpdir="$(mktemp -d)"
mountpoint="$tmpdir/mount"
dmg="$tmpdir/Codex.dmg"

cleanup() {
  if /usr/bin/mount | /usr/bin/grep -q "$mountpoint"; then
    /usr/bin/hdiutil detach "$mountpoint" -quiet || true
  fi
  rm -rf "$tmpdir"
}
trap cleanup EXIT

mkdir -p "$mountpoint"

echo "Downloading Codex app..."
curl -fL --retry 3 -o "$dmg" "$codex_dmg_url"

actual_sha256="$(/usr/bin/shasum -a 256 "$dmg" | /usr/bin/awk '{print $1}')"
if [ "$actual_sha256" != "$expected_sha256" ]; then
  echo "Codex.dmg SHA-256 mismatch"
  echo "expected: $expected_sha256"
  echo "actual:   $actual_sha256"
  exit 1
fi

/usr/bin/hdiutil attach "$dmg" -mountpoint "$mountpoint" -nobrowse -quiet

if [ ! -d "$mountpoint/Codex.app" ]; then
  echo "Codex app was not found in downloaded DMG"
  exit 1
fi

/usr/bin/ditto "$mountpoint/Codex.app" "$codex_app"
echo "Codex app installed"
