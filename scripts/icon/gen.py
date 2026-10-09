#!/usr/bin/env python3
"""The Meetings app icon (F9-FR2): writes the SVG sources next to this file.

    python3 -I scripts/icon/gen.py && scripts/icon/render.sh

The mark: a desk calendar block with depth and a studio mic in front of it, in inkread's Inkwell
language: a few bold ink shapes, details cut in paper, separations in the background colour. It is
drawn on a 432 x 432 canvas, and FIT centres its ink box (about 290 x 280 px) on the canvas. The
Supernote's app list shows the adaptive icon as a 70 px square tile with no circular mask, cropping
the canvas to about its centre 307 px, so the mark fills the tile the way Kindle's picture fills its
own. Like inkread's, the corners of the ink box reach past the 66 dp safe circle.

Outputs:
  meetings-fg-light.svg  launcher foreground (transparent) and the README's light icon
  meetings-fg-dark.svg   the README's dark-mode icon
  legacy-square.svg      the pre-adaptive ic_launcher: the mark on a rounded square of the
                         launcher background, its ink box 72% of the width (inkread's proportion)
  legacy-round.svg       the same on a circle (ic_launcher_round), the ink box 66% of the width
"""
import os

BG = "#F2ECDE"  # = res/values/ic_launcher_background.xml, as inkread
FIT = "translate(4 3)"
# The legacy icons rescale the fitted mark (ink box about 290 px wide) to inkread's measured
# proportions: 72% of the width on the square (x 1.07), 66% inside the circle (x 0.98).
def legacy_scale(k):
    return f"translate(216 216) scale({k}) translate(-216 -216)"

def mark(bg, ink, paper):
    return f'''  <!-- Meetings mark: a desk calendar block with depth and a studio mic in front of it.
       Inkwell language, as inkread's mark: a few bold ink shapes and large paper areas, separations in the
       background colour, simple enough to read on the 70 px launcher tile and the 56 dp home logo. -->
  <!-- Calendar block: extruded right and bottom faces, one bold page edge on each, cut in paper -->
  <path d="M336 104 L356 122 L356 334 L336 316 Z" fill="{ink}"/>
  <path d="M128 316 L336 316 L356 334 L148 334 Z" fill="{ink}"/>
  <g stroke="{paper}" stroke-width="6" stroke-linecap="round" fill="none">
    <line x1="160" y1="326" x2="338" y2="326"/>
    <line x1="346" y1="136" x2="346" y2="314"/>
  </g>
  <!-- Front face: an ink header band over a large paper page -->
  <rect x="128" y="96" width="208" height="220" rx="18" fill="{ink}" stroke="{bg}" stroke-width="10"/>
  <rect x="140" y="150" width="184" height="154" rx="10" fill="{paper}"/>
  <!-- Four days: three outlined, today solid -->
  <g fill="none" stroke="{ink}" stroke-width="9">
    <rect x="190" y="172" width="50" height="44" rx="8"/><rect x="256" y="172" width="50" height="44" rx="8"/>
    <rect x="190" y="238" width="50" height="44" rx="8"/>
  </g>
  <rect x="252" y="234" width="58" height="52" rx="10" fill="{ink}"/>
  <!-- Binder rings -->
  <rect x="172" y="74" width="24" height="52" rx="12" fill="{ink}" stroke="{bg}" stroke-width="10"/>
  <rect x="268" y="74" width="24" height="52" rx="12" fill="{ink}" stroke="{bg}" stroke-width="10"/>

  <!-- Studio microphone in front, over the calendar's left edge: a solid capsule with two grille bars,
       its holder, stem and base, each lifted off the calendar by a band of the background colour -->
  <path d="M74 236 v14 a45 45 0 0 0 90 0 v-14" fill="none" stroke="{bg}" stroke-width="30" stroke-linecap="round"/>
  <path d="M74 236 v14 a45 45 0 0 0 90 0 v-14" fill="none" stroke="{ink}" stroke-width="13" stroke-linecap="round"/>
  <rect x="110" y="292" width="18" height="40" fill="{ink}" stroke="{bg}" stroke-width="10"/>
  <rect x="78" y="328" width="82" height="24" rx="12" fill="{ink}" stroke="{bg}" stroke-width="10"/>
  <rect x="82" y="150" width="74" height="132" rx="37" fill="{ink}" stroke="{bg}" stroke-width="14"/>
  <g stroke="{paper}" stroke-width="9" stroke-linecap="round">
    <line x1="102" y1="192" x2="136" y2="192"/><line x1="102" y1="218" x2="136" y2="218"/>
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
write("legacy-square.svg", legacy(f'<rect width="432" height="432" rx="86" fill="{BG}"/>', 1.07))
write("legacy-round.svg", legacy(f'<circle cx="216" cy="216" r="216" fill="{BG}"/>', 0.98))
