{ pkgs, lib, assetsPath, hostConfig, ... }:

{
  home.packages = lib.optionals (hostConfig.hostName == "eulogia") [
    pkgs.vscode
  ];

  home.activation.configureEditors = lib.hm.dag.entryAfter [ "writeBoundary" ] ''
    set -euo pipefail

    merge_settings() {
      target="$1"
      source="$2"
      mkdir -p "$(dirname "$target")"
      if [ ! -f "$target" ]; then
        printf '{}\n' > "$target"
      fi

      tmp="$(mktemp)"
      current="$(mktemp)"
      ${pkgs.gnused}/bin/sed '/^[[:space:]]*\/\//d' "$target" > "$current"
      if ${pkgs.jq}/bin/jq -S -s '.[0] * .[1]' "$current" "$source" > "$tmp"; then
        if ! cmp -s "$tmp" "$target"; then
          cp "$target" "$target.backup-before-nix-editors"
          mv "$tmp" "$target"
        else
          rm "$tmp"
        fi
      else
        rm -f "$tmp"
        echo "Could not merge editor settings at $target"
      fi
      rm -f "$current"
    }

    merge_keybindings() {
      target="$1"
      source="$2"
      mkdir -p "$(dirname "$target")"
      if [ ! -f "$target" ]; then
        printf '[]\n' > "$target"
      fi

      tmp="$(mktemp)"
      current="$(mktemp)"
      ${pkgs.gnused}/bin/sed '/^[[:space:]]*\/\//d' "$target" > "$current"
      if ${pkgs.jq}/bin/jq -S -s '.[0] + .[1] | unique_by((.key // "") + "\u0000" + (.command // "") + "\u0000" + (.when // ""))' "$current" "$source" > "$tmp"; then
        if ! cmp -s "$tmp" "$target"; then
          cp "$target" "$target.backup-before-nix-editors"
          mv "$tmp" "$target"
        else
          rm "$tmp"
        fi
      else
        rm -f "$tmp"
        echo "Could not merge editor keybindings at $target"
      fi
      rm -f "$current"
    }

    merge_settings "$HOME/Library/Application Support/Code/User/settings.json" "${assetsPath}/editors/vscode/settings.json"
    merge_keybindings "$HOME/Library/Application Support/Code/User/keybindings.json" "${assetsPath}/editors/vscode/keybindings.json"

    merge_settings "$HOME/Library/Application Support/Cursor/User/settings.json" "${assetsPath}/editors/cursor/settings.json"
    merge_keybindings "$HOME/Library/Application Support/Cursor/User/keybindings.json" "${assetsPath}/editors/cursor/keybindings.json"
  '';
}
