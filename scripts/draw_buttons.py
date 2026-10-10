#!/usr/bin/env python3
# Ported from sn-canvas `scripts/draw_features.py` and `scripts/draw_hints.py` (MIT License,
# Copyright (c) 2026 JRaghavan; notice in LICENSES-3RDPARTY.md): the hand-drawn arrow and
# caption are the ones Hints.kt draws on the device, in Virgil (SIL OFL 1.1, app/src/main/assets/fonts).
"""Draws docs/images/notes-buttons.png, F21-FR16's "What every button does": the Notes tab's
docked tool bar as F21-FR2 lays it out (64 dp, the tools top to bottom, the bottom group), with
the Shapes and Insert flyouts open beside it, every control labelled in the hand-drawn style of
the in-app hints.

The bar is drawn from its layout rules (DockedToolBar: 56 dp buttons, 3 dp apart, the icons the
app ships), not from a screenshot, so it can be regenerated on any computer:

    python3 -I scripts/draw_buttons.py        (requires Pillow and rsvg-convert)
"""
import math
import re
import subprocess
import tempfile
from pathlib import Path

from PIL import Image, ImageDraw, ImageFont

ROOT = Path(__file__).resolve().parent.parent
RES = ROOT / "app" / "src" / "main" / "res"
FONT = ROOT / "app" / "src" / "main" / "assets" / "fonts" / "virgil.ttf"
OUT = ROOT / "docs" / "images" / "notes-buttons.png"

DP = 2  # pixels per dp in the figure
INK = "#1971c2"
TITLE = "#1e1e1e"
FRAME = "#adb5bd"
BAR_DP, BUTTON_DP, GAP_DP = 64, 56, 3

# The expanded bar, top to bottom (ToolBarModel.items, nothing selected, recording):
# (kind, icon or text, label)
BAR = [
    ("text", "› ‹", "Fold the tools away"),
    ("icon", "drawable-nodpi/ic_cv_tool_select.png", "Select: tap, drag a box, move"),
    ("chip", "drawable/ic_tool_pen.xml", "Pencil: pressure, colour & size"),
    ("icon", "drawable/ic_tool_highlighter.xml", "Highlighter"),
    ("icon", "drawable/ic_tool_eraser.xml", "Eraser (tap again to clear the canvas)"),
    ("icon", "drawable/ic_tool_lasso.xml", "Lasso: circle ink and items"),
    ("fly", "drawable-nodpi/ic_cv_tool_rectangle.png", "Shapes: rectangle, ellipse, line, arrow"),
    ("fly", "drawable-nodpi/ic_cv_tool_image.png", "Insert: text, sticky notes, tables, images"),
    ("style", None, "Style: colour, opacity, fill, outline, size"),
    ("icon", "drawable/ic_sel_undo.xml", "Undo"),
    ("icon", "drawable/ic_sel_redo.xml", "Redo"),
    ("icon", "drawable/ic_tool_listen.xml", "Listen: tap ink to hear it"),
    ("icon", "drawable-nodpi/ic_cv_action_more.png", "More: canvases, PDF, clear, zoom"),
    ("text", "?", "Show these hints again"),
]
BOTTOM = [
    ("text", "0:12:41", "The recording's time"),
    ("pill", "Mark", "Mark this moment"),
    ("pill", "Stop", "Stop recording"),
]
SHAPES = [("drawable-nodpi/ic_cv_tool_rectangle.png", "Rectangle"), ("drawable-nodpi/ic_cv_tool_ellipse.png", "Ellipse"),
          ("drawable-nodpi/ic_cv_tool_line.png", "Line"), ("drawable-nodpi/ic_cv_action_arrow.png", "Arrow")]
INSERT = [("drawable-nodpi/ic_cv_tool_text.png", "Text box"), ("drawable-nodpi/ic_cv_tool_note.png", "Sticky note"),
          ("drawable-nodpi/ic_cv_tool_table.png", "Table"), ("drawable-nodpi/ic_cv_tool_image.png", "Image")]


def vector_png(xml: Path, px: int) -> Image.Image:
    """An Android vector drawable rendered through rsvg-convert."""
    src = xml.read_text()
    vw = re.search(r'viewportWidth="([\d.]+)"', src).group(1)
    vh = re.search(r'viewportHeight="([\d.]+)"', src).group(1)
    paths = []
    for m in re.finditer(r"<path(.*?)/>", src, re.S):
        a = m.group(1)
        d = re.search(r'pathData="([^"]+)"', a).group(1)
        stroke = re.search(r'strokeWidth="([\d.]+)"', a)
        fill = "none" if 'fillColor="#00000000"' in a or "fillColor" not in a else "black"
        attrs = f'd="{d}" fill="{fill}"'
        if stroke:
            attrs += f' stroke="black" stroke-width="{stroke.group(1)}" stroke-linecap="round"'
        paths.append(f"<path {attrs}/>")
    svg = f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {vw} {vh}" width="{px}" height="{px}">{"".join(paths)}</svg>'
    with tempfile.TemporaryDirectory() as t:
        s, p = Path(t) / "i.svg", Path(t) / "i.png"
        s.write_text(svg)
        subprocess.run(["rsvg-convert", "-w", str(px), "-h", str(px), "-o", str(p), str(s)], check=True)
        return Image.open(p).convert("RGBA").copy()


def icon(rel: str, px: int, white: bool = False) -> Image.Image:
    path = RES / rel
    im = vector_png(path, px) if path.suffix == ".xml" else Image.open(path).convert("RGBA").resize((px, px), Image.LANCZOS)
    if white:
        r, g, b, a = im.split()
        im = Image.merge("RGBA", (r.point(lambda _: 255), g.point(lambda _: 255), b.point(lambda _: 255), a))
    return im


def cubic(p0, p1, p2, p3, t):
    u = 1 - t
    return tuple(u**3 * a + 3 * u * u * t * b + 3 * u * t * t * c + t**3 * d for a, b, c, d in zip(p0, p1, p2, p3))


def arrow(draw, tail, tip, width):
    """Hints.arrow: leaves the caption sideways, reaches the control head-on, wobbling a little."""
    p1 = (tail[0] + 0.75 * (tip[0] - tail[0]), tail[1])
    p2 = (tip[0], tip[1] - 0.45 * (tip[1] - tail[1]))
    pts = []
    for i in range(49):
        t = i / 48
        x, y = cubic(tail, p1, p2, tip, t)
        ax, ay = cubic(tail, p1, p2, tip, max(t - 0.01, 0))
        bx, by = cubic(tail, p1, p2, tip, min(t + 0.01, 1))
        n = math.hypot(bx - ax, by - ay) or 1
        w = 0.9 * DP * math.sin(math.pi * t) * math.sin(2.6 * math.pi * t + 0.4)
        pts.append((x - (by - ay) / n * w, y + (bx - ax) / n * w))
    draw.line(pts, fill=INK, width=width, joint="curve")
    dx, dy = tip[0] - pts[-4][0], tip[1] - pts[-4][1]
    n = math.hypot(dx, dy) or 1
    for s in (1, -1):
        a = s * math.radians(28)
        bx, by = -dx / n, -dy / n
        hx = tip[0] + 10 * DP * (bx * math.cos(a) - by * math.sin(a))
        hy = tip[1] + 10 * DP * (bx * math.sin(a) + by * math.cos(a))
        draw.line([tip, (hx, hy)], fill=INK, width=width)


def main():
    font = ImageFont.truetype(str(FONT), 17 * DP)
    title = ImageFont.truetype(str(FONT), 30 * DP)
    small = ImageFont.truetype(str(FONT), 13 * DP)
    ui = ImageFont.truetype(str(FONT), 12 * DP)
    W, H = 1060 * DP, 1180 * DP
    img = Image.new("RGB", (W, H), "white")
    d = ImageDraw.Draw(img)
    d.text((40 * DP, 28 * DP), "Notes: what every button does", font=title, fill=TITLE)

    # The canvas edge and the bar on the right, as F17-FR2 docks it.
    bar_x = 520 * DP
    top = 110 * DP
    d.rectangle([40 * DP, top, bar_x, H - 40 * DP], outline=FRAME, width=2)
    d.text((55 * DP, top + 12 * DP), "the canvas (it has no edges)", font=small, fill=FRAME)
    d.line([bar_x, top, bar_x, H - 40 * DP], fill="black", width=2)
    bx = bar_x + (BAR_DP - BUTTON_DP) // 2 * DP
    y = top + 8 * DP
    tips = []

    def button(kind, what):
        nonlocal y
        h = BUTTON_DP * DP if kind in ("icon", "chip", "fly", "style") else (52 if kind == "pill" else 30) * DP
        box = [bx, y, bx + BUTTON_DP * DP, y + h]
        if kind in ("icon", "chip", "fly", "style"):
            d.rounded_rectangle(box, radius=9 * DP, outline="black", width=2, fill="black" if kind == "chip" else "white")
        if kind in ("icon", "chip", "fly"):
            im = icon(what, 34 * DP if "cv" in what else 40 * DP, white=kind == "chip")
            img.paste(im, (bx + (BUTTON_DP * DP - im.width) // 2, y + (h - im.height) // 2), im)
            if kind == "fly":  # the small marker pointing where the flyout opens
                tx, ty = bx + 7 * DP, y + h - 9 * DP
                d.polygon([(tx, ty), (tx + 6 * DP, ty - 4 * DP), (tx + 6 * DP, ty + 4 * DP)], fill="black")
        elif kind == "style":
            c = (bx + BUTTON_DP * DP // 2, y + h // 2)
            d.ellipse([c[0] - 11 * DP, c[1] - 11 * DP, c[0] + 11 * DP, c[1] + 11 * DP], fill="black", outline="black", width=2)
        elif kind == "pill":
            d.rounded_rectangle(box, radius=26 * DP, outline="black", width=2, fill="black" if "Stop" in what else "white")
            d.multiline_text(((box[0] + box[2]) / 2 - 1, (box[1] + box[3]) / 2), what, font=ui, anchor="mm",
                             align="center", fill="white" if "Stop" in what else "black")
        else:
            d.text(((box[0] + box[2]) / 2, (box[1] + box[3]) / 2), what, font=ui, anchor="mm", fill="black")
        tips.append((bx + BUTTON_DP * DP + 4 * DP, y + h / 2))
        y += h + 2 * GAP_DP * DP
        return box

    boxes = [button(k, w) for k, w, _ in BAR]
    y = H - 40 * DP - (30 + 52 + 52 + 6 * GAP_DP + 10) * DP
    boxes += [button(k, w) for k, w, _ in BOTTOM]
    labels = [lab for _, _, lab in BAR + BOTTOM]

    # Captions on the right, beyond the screen's edge, each arrow ending on its control.
    for i, (tip, lab) in enumerate(zip(tips, labels)):
        cy = 130 * DP + i * ((H - 190 * DP) / (len(labels) - 1))
        tail = (bar_x + (BAR_DP + 90) * DP, cy)
        d.text((tail[0] + 8 * DP, cy - 12 * DP), lab, font=font, fill=INK)
        arrow(d, tail, tip, 2 * DP // 1)

    # The Shapes and Insert flyouts, open beside the bar on the canvas side (F21-FR2).
    # The Shapes and Insert flyouts open beside the bar, on the canvas side (F21-FR2).
    fx = bar_x - 84 * DP
    for (rows, at, name, dy) in ((SHAPES, boxes[6], "Shapes", -40), (INSERT, boxes[7], "Insert", 250)):
        fy = at[1] + dy * DP
        d.text((fx - 230 * DP, fy - 24 * DP), name + " opens beside the bar:", font=small, fill=INK)
        d.rounded_rectangle([fx, fy, fx + 68 * DP, fy + len(rows) * 60 * DP + 8 * DP], radius=10 * DP, outline="black", width=2,
                            fill="white")
        for j, (rel, lab) in enumerate(rows):
            iy = fy + 4 * DP + j * 60 * DP
            d.rounded_rectangle([fx + 6 * DP, iy, fx + 62 * DP, iy + 56 * DP], radius=9 * DP, outline="black", width=2)
            im = icon(rel, 34 * DP)
            img.paste(im, (fx + 6 * DP + (56 * DP - im.width) // 2, iy + (56 * DP - im.height) // 2), im)
            tw = d.textlength(lab, font=font)
            d.text((fx - 12 * DP - tw, iy + 16 * DP), lab, font=font, fill=INK)
    d.multiline_text((60 * DP, H - 150 * DP), "One panel opens at a time.\nA finger pans and zooms;\nthe pen does the rest.\nTurn the pen over to erase.",
                     font=font, fill=INK)
    OUT.parent.mkdir(parents=True, exist_ok=True)
    img.save(OUT, optimize=True)
    print(OUT)


if __name__ == "__main__":
    main()
