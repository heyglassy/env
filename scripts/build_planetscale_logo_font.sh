#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
source_svg="${1:-$repo_root/assets/logos/planetscale-logo-mark-source.svg}"
font_dir="$repo_root/assets/fonts"
fontforge_script="$(mktemp)"
metrics_ttx="$(mktemp)"

mkdir -p "$font_dir"

cleanup() {
  rm -f "$fontforge_script" "$metrics_ttx"
}
trap cleanup EXIT

cat > "$fontforge_script" <<PY
import fontforge
import psMat

font = fontforge.font()
font.encoding = "UnicodeFull"
font.fontname = "PlanetScaleLogo-Regular"
font.familyname = "PlanetScale Logo"
font.fullname = "PlanetScale Logo Regular"
font.version = "1.0"
font.em = 1000
font.ascent = 800
font.descent = 200
font.os2_xheight = 522
font.os2_capheight = 680

space = font.createChar(0x20, "space")
space.width = 600

latin_a = font.createChar(0x41, "A")
pen = latin_a.glyphPen()
pen.moveTo((80, 0))
pen.lineTo((190, 0))
pen.lineTo((300, 520))
pen.lineTo((410, 0))
pen.lineTo((520, 0))
pen.lineTo((360, 680))
pen.lineTo((240, 680))
pen.closePath()
pen.moveTo((225, 260))
pen.lineTo((375, 260))
pen.lineTo((345, 370))
pen.lineTo((255, 370))
pen.closePath()
latin_a.width = 600
latin_a.removeOverlap()
latin_a.correctDirection()

glyph = font.createChar(0xE401, "uniE401")
glyph.importOutlines("$source_svg")
glyph.removeOverlap()
glyph.correctDirection()

bounds = glyph.boundingBox()
width = bounds[2] - bounds[0]
height = bounds[3] - bounds[1]
target_width = 600
target_height = 600
scale = min(target_width / width, target_height / height)
glyph.transform(psMat.translate(-bounds[0], -bounds[1]))
glyph.transform(psMat.scale(scale))
bounds = glyph.boundingBox()
glyph_width = bounds[2] - bounds[0]
glyph_height = bounds[3] - bounds[1]
glyph.transform(psMat.translate((600 - glyph_width) / 2, (font.os2_capheight - glyph_height) / 2 - bounds[1]))
glyph.width = 600

font.hhea_ascent = 956
font.hhea_descent = -244
font.hhea_linegap = 0
font.os2_typoascent = 956
font.os2_typodescent = -244
font.os2_typolinegap = 0
font.os2_winascent = 956
font.os2_windescent = 244

font.generate("$font_dir/PlanetScaleLogo-Regular.ttf")
PY

fontforge -lang=py -script "$fontforge_script"

ttx -q -o "$metrics_ttx" "$font_dir/PlanetScaleLogo-Regular.ttf"
perl -0pi -e '
  s#<ascent value="[^"]+"\s*/>#<ascent value="956"/>#;
  s#<descent value="[^"]+"\s*/>#<descent value="-244"/>#;
  s#<lineGap value="[^"]+"\s*/>#<lineGap value="0"/>#;
  s#<sTypoAscender value="[^"]+"\s*/>#<sTypoAscender value="956"/>#;
  s#<sTypoDescender value="[^"]+"\s*/>#<sTypoDescender value="-244"/>#;
  s#<sTypoLineGap value="[^"]+"\s*/>#<sTypoLineGap value="0"/>#;
  s#<usWinAscent value="[^"]+"\s*/>#<usWinAscent value="956"/>#;
  s#<usWinDescent value="[^"]+"\s*/>#<usWinDescent value="244"/>#;
  s#<sxHeight value="[^"]+"\s*/>#<sxHeight value="522"/>#;
  s#<sCapHeight value="[^"]+"\s*/>#<sCapHeight value="680"/>#;
' "$metrics_ttx"
ttx -q -o "$font_dir/PlanetScaleLogo-Regular.ttf" "$metrics_ttx"

echo "Wrote $font_dir/PlanetScaleLogo-Regular.ttf"
