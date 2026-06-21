{ lib, assetsPath, hostConfig, ... }:

let
  userHome = "/Users/${hostConfig.userName}";
  privateFontSources = hostConfig.privateFontSources or [
    "${userHome}/.config/nix/darwin-private/fonts-berkeley-mono"
    "${userHome}/.config/nix/darwin-private/fonts/berkeley-mono"
    "${userHome}/.config/nix-darwin-private/fonts/berkeley-mono"
  ];
  privateFontSourceEntries = lib.concatMapStringsSep "\n" (source: ''
      "${source}"'') privateFontSources;
  insignaLogoFont = assetsPath + "/fonts/InsignaLogo-Regular.ttf";
  planetScaleLogoFont = assetsPath + "/fonts/PlanetScaleLogo-Regular.ttf";
in
{
  home.activation.installPrivateFonts = lib.hm.dag.entryAfter [ "writeBoundary" ] ''
    font_sources=(
${privateFontSourceEntries}
    )
    target_dir="${userHome}/Library/Fonts"
    preferred_source_dir="''${font_sources[0]}"

    source_dir=""
    for candidate in "''${font_sources[@]}"; do
      [ -d "$candidate" ] || continue
      for font in "$candidate"/BerkeleyMono*.otf "$candidate"/BerkeleyMono*.ttf; do
        [ -e "$font" ] || continue
        source_dir="$candidate"
        break 2
      done
    done

    if [ -z "$source_dir" ]; then
      mkdir -p "$preferred_source_dir"
      if [ "$(id -u)" -eq 0 ]; then
        chown "${hostConfig.userName}:staff" "$preferred_source_dir" 2>/dev/null || true
      fi
      echo "Berkeley Mono font files not found. Created private font directory:"
      echo "  $preferred_source_dir"
      echo "Copy licensed BerkeleyMono*.otf/.ttf files there, then rerun switch."
      echo "Checked:"
      printf '  %s\n' "''${font_sources[@]}"
      exit 0
    fi

    mkdir -p "$target_dir"
    copied=0
    for font in "$source_dir"/BerkeleyMono*.otf "$source_dir"/BerkeleyMono*.ttf; do
      [ -e "$font" ] || continue
      target="$target_dir/$(basename "$font")"
      if [ ! -e "$target" ] || ! cmp -s "$font" "$target"; then
        cp "$font" "$target"
        copied=1
      fi
    done

    if [ "$copied" -eq 1 ]; then
      echo "Installed or updated Berkeley Mono fonts"
    fi
  '';

  home.activation.installInsignaLogoFont = lib.hm.dag.entryAfter [ "writeBoundary" ] ''
    source_font="${insignaLogoFont}"
    target_font="$HOME/Library/Fonts/InsignaLogo-Regular.ttf"
    target_dir="$(dirname "$target_font")"

    mkdir -p "$target_dir"
    if [ -L "$target_font" ] || [ ! -e "$target_font" ] || ! cmp -s "$source_font" "$target_font"; then
      rm -f "$target_font"
      cp "$source_font" "$target_font"
      echo "Installed or updated Insigna Logo font"
      killall fontd 2>/dev/null || true
    fi
  '';

  home.activation.installPlanetScaleLogoFont = lib.hm.dag.entryAfter [ "writeBoundary" ] ''
    source_font="${planetScaleLogoFont}"
    target_font="$HOME/Library/Fonts/PlanetScaleLogo-Regular.ttf"
    target_dir="$(dirname "$target_font")"

    mkdir -p "$target_dir"
    if [ -L "$target_font" ] || [ ! -e "$target_font" ] || ! cmp -s "$source_font" "$target_font"; then
      rm -f "$target_font"
      cp "$source_font" "$target_font"
      echo "Installed or updated PlanetScale Logo font"
      killall fontd 2>/dev/null || true
    fi
  '';
}
