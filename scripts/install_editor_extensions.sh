#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

install_extensions() {
  local cli="$1"
  local extension_file="$2"

  if [ ! -x "$cli" ]; then
    echo "Editor CLI not found at $cli; skipping $(basename "$(dirname "$extension_file")") extensions"
    return
  fi

  local installed
  installed="$("$cli" --list-extensions 2>/dev/null | tr '[:upper:]' '[:lower:]' || true)"

  while IFS= read -r extension || [ -n "$extension" ]; do
    [ -n "$extension" ] || continue
    case "$extension" in \#*) continue ;; esac

    extension_lower="$(printf '%s' "$extension" | tr '[:upper:]' '[:lower:]')"
    if ! printf '%s\n' "$installed" | grep -Fxq "$extension_lower"; then
      "$cli" --install-extension "$extension" --force >/dev/null || echo "Failed to install editor extension $extension"
    fi
  done < "$extension_file"
}

install_extensions \
  "/Applications/Visual Studio Code.app/Contents/Resources/app/bin/code" \
  "$repo_root/assets/editors/vscode/extensions.txt"

install_extensions \
  "/Applications/Cursor.app/Contents/Resources/app/bin/cursor" \
  "$repo_root/assets/editors/cursor/extensions.txt"
