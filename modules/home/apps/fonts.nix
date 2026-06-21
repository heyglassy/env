{ lib, assetsPath, hostConfig, ... }:

let
  privateFontSource = "/Users/${hostConfig.userName}/.config/nix-darwin-private/fonts/berkeley-mono";
  insignaLogoFont = assetsPath + "/fonts/InsignaLogo-Regular.ttf";
  planetScaleLogoFont = assetsPath + "/fonts/PlanetScaleLogo-Regular.ttf";
in
{
  home.activation.installPrivateFonts = lib.hm.dag.entryAfter [ "writeBoundary" ] ''
    source_dir="${privateFontSource}"
    target_dir="$HOME/Library/Fonts"

    if [ ! -d "$source_dir" ]; then
      echo "Berkeley Mono font source not found at $source_dir"
      echo "Place licensed Berkeley Mono .otf/.ttf files there, then rerun switch."
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
