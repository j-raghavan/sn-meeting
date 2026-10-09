#!/usr/bin/env python3
"""The Meetings app icon (F9-FR2): writes the SVG sources next to this file.

    python3 -I scripts/icon/gen.py && scripts/icon/render.sh

The mark (the approved design): a desk calendar block with depth, a studio mic in front and a
fountain pen leaning on it, in inkread's Inkwell language: solid ink shapes, details cut in
paper, separations in the background colour. It is drawn on a 432 x 432 canvas and fitted into
the adaptive-icon safe zone (FIT): its bounding box, centred at (222, 204), is moved to the
canvas centre and scaled by 0.8625, an ink box of about 265 x 198 px: the same footprint as
inkread's mark (250 x 229 px), so the two icons carry the same weight on the Supernote's app
list, which shows the full square tile without a circular mask. Like inkread's, the corners of
the ink box reach slightly past the 66 dp safe circle.

Outputs:
  meetings-fg-light.svg  launcher foreground (transparent) and the README's light icon
  meetings-fg-dark.svg   the README's dark-mode icon
  legacy-square.svg      the pre-adaptive ic_launcher: the mark on a rounded square of the
                         launcher background, its ink box 68% of the width (inkread's margin)
  legacy-round.svg       the same on a circle (ic_launcher_round), the ink box 62% of the width
"""
import os

BG = "#F2ECDE"  # = res/values/ic_launcher_background.xml, as inkread
FIT = "translate(216 216) scale(0.8625) translate(-222 -204)"
# The legacy icons rescale the fitted mark (ink box 297 x 0.8625 = 256 px wide) to inkread's
# margin: 68% of the width on the square (x 1.148), 62% inside the circle (x 1.043).
def legacy_scale(k):
    return f"translate(216 216) scale({k}) translate(-216 -216)"

def mark(bg, ink, paper):
    return f'''  <!-- Meetings mark: a desk calendar block with depth, a studio mic in front, a fountain pen leaning on it.
       Inkwell language, as inkread's mark: solid ink shapes, details cut in paper, separations in the background colour. -->
  <!-- Calendar block: extruded right and bottom faces (depth 22,18), page edges cut in paper -->
  <path d="M290 112 L312 130 L312 290 L290 272 Z" fill="{ink}"/>
  <path d="M112 272 L290 272 L312 290 L134 290 Z" fill="{ink}"/>
  <path d="M134 290 L312 290 L312 130" fill="none" stroke="{ink}" stroke-width="10" stroke-linejoin="round"/>
  <g stroke="{paper}" stroke-width="3" stroke-linecap="round" fill="none">
    <line x1="140" y1="279" x2="292" y2="279"/>
    <line x1="146" y1="285" x2="298" y2="285"/>
    <line x1="299" y1="140" x2="299" y2="270"/>
    <line x1="305" y1="146" x2="305" y2="276"/>
  </g>
  <!-- Front face -->
  <rect x="112" y="112" width="178" height="160" rx="14" fill="{ink}" stroke="{bg}" stroke-width="6"/>
  <!-- Page under the header band -->
  <rect x="122" y="158" width="158" height="104" rx="8" fill="{paper}"/>
  <!-- Date grid 3 x 2, one day ringed (chunky so it reads at launcher size) -->
  <g fill="{ink}">
    <rect x="138" y="172" width="34" height="30" rx="6"/><rect x="184" y="172" width="34" height="30" rx="6"/>
    <rect x="230" y="172" width="34" height="30" rx="6"/>
    <rect x="138" y="214" width="34" height="30" rx="6"/><rect x="230" y="214" width="34" height="30" rx="6"/>
  </g>
  <rect x="180" y="210" width="42" height="38" rx="10" fill="none" stroke="{ink}" stroke-width="7"/>
  <rect x="190" y="220" width="22" height="18" rx="4" fill="{ink}"/>
  <!-- Binder rings -->
  <rect x="144" y="92" width="18" height="42" rx="9" fill="{ink}" stroke="{bg}" stroke-width="6"/>
  <rect x="240" y="92" width="18" height="42" rx="9" fill="{ink}" stroke="{bg}" stroke-width="6"/>
  <rect x="149" y="124" width="8" height="10" rx="4" fill="{paper}"/>
  <rect x="245" y="124" width="8" height="10" rx="4" fill="{paper}"/>

  <!-- Studio microphone, front left -->
  <path d="M78 212 v14 a36 36 0 0 0 72 0 v-14" fill="none" stroke="{bg}" stroke-width="22" stroke-linecap="round"/>
  <path d="M78 212 v14 a36 36 0 0 0 72 0 v-14" fill="none" stroke="{ink}" stroke-width="10" stroke-linecap="round"/>
  <rect x="106" y="260" width="16" height="44" fill="{ink}" stroke="{bg}" stroke-width="6"/>
  <rect x="78" y="298" width="72" height="18" rx="9" fill="{ink}" stroke="{bg}" stroke-width="6"/>
  <rect x="88" y="144" width="52" height="96" rx="26" fill="{ink}" stroke="{bg}" stroke-width="7"/>
  <g stroke="{paper}" stroke-width="4" stroke-linecap="round">
    <line x1="100" y1="168" x2="128" y2="168"/><line x1="98" y1="182" x2="130" y2="182"/>
    <line x1="98" y1="196" x2="130" y2="196"/><line x1="100" y1="210" x2="128" y2="210"/>
  </g>

  <!-- Fountain pen leaning across the right -->
  <g transform="rotate(32 304 236)">
    <rect x="290" y="128" width="30" height="134" rx="13" fill="{ink}" stroke="{bg}" stroke-width="7"/>
    <rect x="290" y="166" width="30" height="7" fill="{paper}"/>
    <rect x="313" y="134" width="7" height="34" rx="3" fill="{paper}"/>
    <path d="M292 262 L318 262 L305 312 Z" fill="{ink}" stroke="{bg}" stroke-width="6" stroke-linejoin="round"/>
    <line x1="305" y1="274" x2="305" y2="298" stroke="{paper}" stroke-width="3" stroke-linecap="round"/>
    <circle cx="305" cy="274" r="3.5" fill="{paper}"/>
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
write("legacy-square.svg", legacy(f'<rect width="432" height="432" rx="86" fill="{BG}"/>', 1.148))
write("legacy-round.svg", legacy(f'<circle cx="216" cy="216" r="216" fill="{BG}"/>', 1.043))
