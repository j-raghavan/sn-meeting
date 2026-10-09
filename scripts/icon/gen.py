#!/usr/bin/env python3
"""The Meetings app icon (F9-FR2): writes the SVG sources next to this file.

    python3 -I scripts/icon/gen.py && scripts/icon/render.sh

The mark (the approved design): a desk calendar block with depth, a studio mic in front and a
fountain pen leaning on it, in inkread's Inkwell language: solid ink shapes, details cut in
paper, separations in the background colour. It is drawn on a 432 x 432 canvas and fitted into
the adaptive-icon safe zone (FIT): its bounding box, centred at (222, 204), is moved to the
canvas centre at full size: the mic and pen are tucked in so the ink box is 250 x 230 px, the same
footprint as inkread's mark (250 x 229 px). The two icons carry the same weight on the Supernote's
app list, which shows the full square tile without a circular mask. Like inkread's, the corners of
the ink box reach slightly past the 66 dp safe circle.

Outputs:
  meetings-fg-light.svg  launcher foreground (transparent) and the README's light icon
  meetings-fg-dark.svg   the README's dark-mode icon
  legacy-square.svg      the pre-adaptive ic_launcher: the mark on a rounded square of the
                         launcher background, its ink box 72% of the width (inkread's proportion)
  legacy-round.svg       the same on a circle (ic_launcher_round), the ink box 66% of the width
"""
import os

BG = "#F2ECDE"  # = res/values/ic_launcher_background.xml, as inkread
FIT = "translate(216 216) scale(1) translate(-218 -204)"
# The legacy icons rescale the fitted mark (ink box 250 px wide) to inkread's measured
# proportions: 72% of the width on the square (x 1.318), 66% inside the circle (x 1.193).
def legacy_scale(k):
    return f"translate(216 216) scale({k}) translate(-216 -216)"

def mark(bg, ink, paper):
    return f'''  <!-- Meetings mark: a desk calendar block with depth, a studio mic in front, a fountain pen leaning on it.
       Inkwell language, as inkread's mark: a few bold ink shapes and large paper areas, separations in the
       background colour, thick enough to survive the 56 dp home-screen logo and the 48 dp launcher tile. -->
  <!-- Calendar block: extruded right and bottom faces (depth 22,18), two bold page edges cut in paper -->
  <path d="M290 112 L312 130 L312 290 L290 272 Z" fill="{ink}"/>
  <path d="M112 272 L290 272 L312 290 L134 290 Z" fill="{ink}"/>
  <path d="M134 290 L312 290 L312 130" fill="none" stroke="{ink}" stroke-width="10" stroke-linejoin="round"/>
  <g stroke="{paper}" stroke-width="5" stroke-linecap="round" fill="none">
    <line x1="146" y1="282" x2="294" y2="282"/>
    <line x1="302" y1="144" x2="302" y2="272"/>
  </g>
  <!-- Front face: an ink header band over a large paper page -->
  <rect x="112" y="112" width="178" height="160" rx="14" fill="{ink}" stroke="{bg}" stroke-width="9"/>
  <rect x="121" y="154" width="160" height="109" rx="8" fill="{paper}"/>
  <!-- Date grid 3 x 2: outlined days, today solid -->
  <g fill="none" stroke="{ink}" stroke-width="6">
    <rect x="139" y="171" width="32" height="28" rx="6"/><rect x="185" y="171" width="32" height="28" rx="6"/>
    <rect x="231" y="171" width="32" height="28" rx="6"/>
    <rect x="139" y="215" width="32" height="28" rx="6"/><rect x="231" y="215" width="32" height="28" rx="6"/>
  </g>
  <rect x="182" y="212" width="38" height="34" rx="7" fill="{ink}"/>
  <!-- Binder rings -->
  <rect x="144" y="92" width="18" height="42" rx="9" fill="{ink}" stroke="{bg}" stroke-width="8"/>
  <rect x="240" y="92" width="18" height="42" rx="9" fill="{ink}" stroke="{bg}" stroke-width="8"/>

  <!-- Studio microphone, front left (tucked over the calendar's left edge for a squarer mark):
       an outlined capsule with three bold grille bars -->
  <g transform="translate(26 0)">
  <path d="M78 212 v14 a36 36 0 0 0 72 0 v-14" fill="none" stroke="{bg}" stroke-width="24" stroke-linecap="round"/>
  <path d="M78 212 v14 a36 36 0 0 0 72 0 v-14" fill="none" stroke="{ink}" stroke-width="11" stroke-linecap="round"/>
  <rect x="106" y="260" width="16" height="44" fill="{ink}" stroke="{bg}" stroke-width="8"/>
  <rect x="78" y="298" width="72" height="18" rx="9" fill="{ink}" stroke="{bg}" stroke-width="8"/>
  <rect x="88" y="144" width="52" height="96" rx="26" fill="{bg}" stroke="{bg}" stroke-width="18"/>
  <rect x="88" y="144" width="52" height="96" rx="26" fill="{paper}" stroke="{ink}" stroke-width="10"/>
  <g stroke="{ink}" stroke-width="7" stroke-linecap="round">
    <line x1="104" y1="172" x2="124" y2="172"/><line x1="104" y1="192" x2="124" y2="192"/>
    <line x1="104" y1="212" x2="124" y2="212"/>
  </g>
  </g>
  <!-- Fountain pen leaning across the right (drawn in closer for a squarer mark) -->
  <g transform="translate(-30 0) rotate(32 304 236)">
    <rect x="290" y="128" width="30" height="134" rx="13" fill="{ink}" stroke="{bg}" stroke-width="9"/>
    <rect x="290" y="164" width="30" height="9" fill="{paper}"/>
    <path d="M292 262 L318 262 L305 312 Z" fill="{ink}" stroke="{bg}" stroke-width="8" stroke-linejoin="round"/>
    <line x1="305" y1="272" x2="305" y2="296" stroke="{paper}" stroke-width="4" stroke-linecap="round"/>
  </g>
'''

def svg(content):
    return ('<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 432 432" width="432" height="432">\n'
            + content + "</svg>\n")

def fg(bg, ink, paper):
    return svg(f'<g transform="{FIT}">\n{mark(bg, ink, paper)}</g>\n')

def legacy(shape, k):
    m = mark(BG, "#111111", "#FFFFFF")
    return svg(f'{shape}\n<g transform="{legacy_scale(k)}"><g transform="{FIT}">\n{m}</g></g>\n')

here = os.path.dirname(os.path.abspath(__file__))
def write(name, text):
    with open(os.path.join(here, name), "w") as f:
        f.write(text)

write("meetings-fg-light.svg", fg(BG, "#111111", "#FFFFFF"))   # launcher + README light
write("meetings-fg-dark.svg", fg("#111111", BG, "#111111"))    # README dark mode
write("legacy-square.svg", legacy(f'<rect width="432" height="432" rx="86" fill="{BG}"/>', 1.318))
write("legacy-round.svg", legacy(f'<circle cx="216" cy="216" r="216" fill="{BG}"/>', 1.193))
