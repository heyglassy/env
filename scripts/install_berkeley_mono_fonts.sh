#!/usr/bin/env bash
set -euo pipefail

document="${BERKELEY_MONO_1P_DOCUMENT:-Berkeley Mono Fonts}"
vault="${BERKELEY_MONO_1P_VAULT:-Personal}"
target_dir="${BERKELEY_MONO_FONT_DIR:-$HOME/.config/nix/darwin-private/fonts-berkeley-mono}"

if ! command -v op >/dev/null 2>&1; then
  echo "1Password CLI is not installed; run switch first so Homebrew installs it."
  exit 1
fi

if ! command -v unzip >/dev/null 2>&1; then
  echo "unzip is not installed."
  exit 1
fi

if ! op whoami >/dev/null 2>&1; then
  echo "1Password CLI is not signed in. Open 1Password or run: op signin"
  exit 1
fi

tmpdir="$(mktemp -d)"
cleanup() {
  rm -rf "$tmpdir"
}
trap cleanup EXIT

zip_file="$tmpdir/berkeley-mono-fonts.zip"
op_args=(document get "$document" --out-file "$zip_file" --force)
if [ -n "$vault" ]; then
  op_args+=(--vault "$vault")
fi

echo "Downloading Berkeley Mono zip from 1Password document: $document"
if ! op "${op_args[@]}"; then
  echo "Could not download the 1Password document."
  echo "Upload /Users/heyglassy/Desktop/berkeley-mono-fonts.zip as a Document titled '$document' in vault '$vault'."
  exit 1
fi

while IFS= read -r entry; do
  case "$entry" in
    BerkeleyMono*.otf|BerkeleyMono*.ttf) ;;
    *)
      echo "Unexpected file in Berkeley Mono zip: $entry"
      echo "Expected only flat BerkeleyMono*.otf or BerkeleyMono*.ttf files."
      exit 1
      ;;
  esac
done < <(unzip -Z1 "$zip_file")

extract_dir="$tmpdir/extracted"
mkdir -p "$extract_dir"
unzip -q "$zip_file" -d "$extract_dir"

mkdir -p "$target_dir"
copied=0
for font in "$extract_dir"/BerkeleyMono*.otf "$extract_dir"/BerkeleyMono*.ttf; do
  [ -e "$font" ] || continue
  cp "$font" "$target_dir/$(basename "$font")"
  copied=$((copied + 1))
done

if [ "$copied" -eq 0 ]; then
  echo "No BerkeleyMono*.otf or BerkeleyMono*.ttf files found in the downloaded zip."
  exit 1
fi

echo "Installed $copied Berkeley Mono source files into:"
echo "  $target_dir"
echo "Run just switch to copy them into ~/Library/Fonts."
