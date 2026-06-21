#!/usr/bin/env bash
set -euo pipefail

codex_app="${CODEX_APP_PATH:-/Applications/Codex.app}"
codex_dmg_url="${CODEX_APP_DMG_URL:-https://persistent.oaistatic.com/codex-app-prod/Codex.dmg}"
expected_sha256="${CODEX_APP_SHA256:-}"
expected_team_id="2DC432GLL2"

if [ -d "$codex_app" ]; then
  echo "Codex app already installed"
  exit 0
fi

tmpdir="$(mktemp -d)"
mountpoint="$tmpdir/mount"
dmg="$tmpdir/Codex.dmg"
attached=0

cleanup() {
  if [ "$attached" -eq 1 ]; then
    /usr/bin/hdiutil detach "$mountpoint" -quiet || true
  fi
  rm -rf "$tmpdir"
}
trap cleanup EXIT

mkdir -p "$mountpoint"

echo "Downloading Codex app..."
curl -fL --retry 3 -o "$dmg" "$codex_dmg_url"

actual_sha256="$(/usr/bin/shasum -a 256 "$dmg" | /usr/bin/awk '{print $1}')"
echo "Codex.dmg SHA-256: $actual_sha256"
if [ -n "$expected_sha256" ] && [ "$actual_sha256" != "$expected_sha256" ]; then
  echo "Codex.dmg SHA-256 mismatch"
  echo "expected: $expected_sha256"
  echo "actual:   $actual_sha256"
  echo "Update CODEX_APP_SHA256 or leave it unset to use app signature validation."
  exit 1
fi

/usr/bin/hdiutil attach "$dmg" -mountpoint "$mountpoint" -nobrowse -quiet
attached=1

if [ ! -d "$mountpoint/Codex.app" ]; then
  echo "Codex app was not found in downloaded DMG"
  exit 1
fi

signature_details="$(/usr/bin/codesign -dv --verbose=4 "$mountpoint/Codex.app" 2>&1)"
if ! /usr/bin/codesign --verify --deep --strict "$mountpoint/Codex.app"; then
  echo "Codex app signature verification failed"
  exit 1
fi

if ! printf '%s\n' "$signature_details" | /usr/bin/grep -q "TeamIdentifier=$expected_team_id"; then
  echo "Codex app was not signed by the expected OpenAI team ID"
  echo "expected team ID: $expected_team_id"
  printf '%s\n' "$signature_details"
  exit 1
fi

/usr/bin/ditto "$mountpoint/Codex.app" "$codex_app"
echo "Codex app installed"
