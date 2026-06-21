# Prompt Logo Fonts

`InsignaLogo-Regular.ttf` contains the Insigna mark as a monochrome glyph at
private-use codepoint `U+E400`.

Rebuild it from the source PNG with:

```sh
nix shell nixpkgs#imagemagick nixpkgs#potrace nixpkgs#fontforge nixpkgs#python313Packages.fonttools -c ./scripts/build_insigna_logo_font.sh
```

Once the font is installed and configured as a terminal fallback, Starship can
render it with:

```toml
[os.symbols]
Macos = "\uE400"
```

`PlanetScaleLogo-Regular.ttf` contains the PlanetScale mark as a monochrome
glyph at private-use codepoint `U+E401`.

Rebuild it from the source SVG with:

```sh
nix shell nixpkgs#fontforge nixpkgs#python313Packages.fonttools -c ./scripts/build_planetscale_logo_font.sh
```
