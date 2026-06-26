#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

install_extensions() {
  local cli="$1"
  local extension_file="$2"
  local editor_name="$3"

  if [ ! -x "$cli" ]; then
    echo "Editor CLI not found for $editor_name; skipping $(basename "$(dirname "$extension_file")") extensions"
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

resolve_cli() {
  local command_name="$1"
  shift

  local cli
  cli="$(command -v "$command_name" 2>/dev/null || true)"
  if [ -n "$cli" ] && [ -x "$cli" ]; then
    printf '%s\n' "$cli"
    return
  fi

  for cli in "$@"; do
    if [ -x "$cli" ]; then
      printf '%s\n' "$cli"
      return
    fi
  done

  return 1
}

install_extensions \
  "$(resolve_cli code \
    "$HOME/Applications/Home Manager Apps/Visual Studio Code.app/Contents/Resources/app/bin/code" \
    "/Applications/Visual Studio Code.app/Contents/Resources/app/bin/code" \
    || true)" \
  "$repo_root/assets/editors/vscode/extensions.txt" \
  "VS Code"

install_extensions \
  "$(resolve_cli cursor \
    "$HOME/Applications/Home Manager Apps/Cursor.app/Contents/Resources/app/bin/cursor" \
    "/Applications/Cursor.app/Contents/Resources/app/bin/cursor" \
    || true)" \
  "$repo_root/assets/editors/cursor/extensions.txt" \
  "Cursor"
