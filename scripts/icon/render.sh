#!/bin/bash
set -euo pipefail
# Render the Meetings icon (F9-FR2) from the SVGs gen.py writes, at inkread's sizes:
#   mipmap-*/ic_launcher_foreground.png  108/162/216/324/432 px (adaptive foreground, 108 dp)
#   mipmap-*/ic_launcher.png, ic_launcher_round.png  48/72/96/144/192 px (legacy, 48 dp)
#   .github/assets/meetings-icon-{light,dark}.png  432 px, transparent (README)
# Needs rsvg-convert (brew install librsvg).
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$HERE/../.." && pwd)"
RES="$ROOT/app/src/main/res"
command -v rsvg-convert >/dev/null || { echo "rsvg-convert not found (brew install librsvg)" >&2; exit 1; }

render() { rsvg-convert -w "$2" -h "$2" "$HERE/$1" -o "$3"; }

for spec in mdpi:1 hdpi:1.5 xhdpi:2 xxhdpi:3 xxxhdpi:4; do
    dpi="${spec%%:*}"; scale="${spec##*:}"
    fg="$(awk -v s="$scale" 'BEGIN { printf "%d", 108 * s }')"
    legacy="$(awk -v s="$scale" 'BEGIN { printf "%d", 48 * s }')"
    mkdir -p "$RES/mipmap-$dpi"
    render meetings-fg-light.svg "$fg" "$RES/mipmap-$dpi/ic_launcher_foreground.png"
    render legacy-square.svg "$legacy" "$RES/mipmap-$dpi/ic_launcher.png"
    render legacy-round.svg "$legacy" "$RES/mipmap-$dpi/ic_launcher_round.png"
done
mkdir -p "$ROOT/.github/assets"
render meetings-fg-light.svg 432 "$ROOT/.github/assets/meetings-icon-light.png"
render meetings-fg-dark.svg 432 "$ROOT/.github/assets/meetings-icon-dark.png"
echo "Rendered the launcher icons and the README icons." >&2
