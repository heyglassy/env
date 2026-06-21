#!/usr/bin/env bash
set -euo pipefail

document="${BERKELEY_MONO_1P_DOCUMENT:-op://Personal/berkeley-mono-fonts/berkeley-mono-fonts.zip}"
vault="${BERKELEY_MONO_1P_VAULT:-Personal}"
target_dir="${BERKELEY_MONO_FONT_DIR:-$HOME/.config/nix/darwin-private/fonts-berkeley-mono}"

find_op() {
  for candidate in \
    /opt/homebrew/bin/op \
    /usr/local/bin/op \
    /run/current-system/sw/bin/op; do
    if [ -x "$candidate" ]; then
      printf '%s\n' "$candidate"
      return 0
    fi
  done

  command -v op 2>/dev/null
}

op_bin="${OP_BIN:-$(find_op || true)}"

if [ -z "$op_bin" ]; then
  brew_bin=""
  for candidate in \
    /opt/homebrew/bin/brew \
    /usr/local/bin/brew; do
    if [ -x "$candidate" ]; then
      brew_bin="$candidate"
      break
    fi
  done

  if [ -n "$brew_bin" ]; then
    echo "1Password CLI is not installed; installing it with Homebrew..."
    "$brew_bin" install --cask 1password-cli
    op_bin="${OP_BIN:-$(find_op || true)}"
  fi
fi

if [ -z "$op_bin" ]; then
  echo "1Password CLI is not installed and Homebrew was not available to install it."
  echo "Run just switch first, then rerun: just install-berkeley-mono"
  exit 1
fi

if ! command -v unzip >/dev/null 2>&1; then
  echo "unzip is not installed."
  exit 1
fi

if ! "$op_bin" whoami >/dev/null 2>&1; then
  echo "1Password CLI is not signed in; attempting op signin..."
  if ! "$op_bin" signin >/dev/null; then
    echo "1Password CLI sign-in failed. Open 1Password or run: op signin"
    exit 1
  fi
fi

tmpdir="$(mktemp -d)"
cleanup() {
  rm -rf "$tmpdir"
}
trap cleanup EXIT

zip_file="$tmpdir/berkeley-mono-fonts.zip"

echo "Downloading Berkeley Mono zip from 1Password: $document"
if [[ "$document" == op://* ]]; then
  op_args=(read --out-file "$zip_file" --force "$document")
else
  op_args=(document get "$document" --out-file "$zip_file" --force)
  if [ -n "$vault" ]; then
    op_args+=(--vault "$vault")
  fi
fi

if ! "$op_bin" "${op_args[@]}"; then
  echo "Could not download the 1Password document."
  echo "Upload /Users/heyglassy/Desktop/berkeley-mono-fonts.zip to 1Password and set BERKELEY_MONO_1P_DOCUMENT to its op:// reference."
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
