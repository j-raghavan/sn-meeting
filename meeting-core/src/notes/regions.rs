//! F17-FR9 / F19-FR6: the canvas's inked area cut into regions, each rendered to a PNG for the
//! Stop call. Strokes closer than [GAP] world units share a region; a region's long edge is at
//! most [`MAX_REGION_EDGE_PX`] at a legible scale (1, or down to [MIN_SCALE] for ink written
//! large), and a bigger cluster is tiled. Black ink on white, 8-bit grayscale.
use crate::ink::{BBox, InkLayer, Stroke, Tool};
use crate::notes_text::{MAX_REGION_EDGE_PX, Rect, RegionImage, StrokeBox};

/// Strokes nearer than this (world units, screen px at 100 %) are one region.
pub const GAP: f32 = 120.0;
/// White space kept round a region's ink.
pub const MARGIN: f32 = 24.0;
/// The smallest scale a region is drawn at: half size still reads (ink written zoomed out).
pub const MIN_SCALE: f32 = 0.5;

const EDGE: f32 = MAX_REGION_EDGE_PX as f32;

/// One region to send: its world rectangle and the scale it is drawn at.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Cut {
    pub rect: BBox,
    pub scale: f32,
}

fn grow(b: BBox, by: f32) -> BBox {
    BBox {
        x0: b.x0 - by,
        y0: b.y0 - by,
        x1: b.x1 + by,
        y1: b.y1 + by,
    }
}

/// The clusters of strokes, as their bounds, in reading order (top to bottom, then left).
pub fn clusters(layer: &InkLayer) -> Vec<BBox> {
    let mut out: Vec<BBox> = Vec::new();
    for s in layer.strokes() {
        let mut b = grow(s.bounds(), GAP / 2.0);
        // Merge every cluster this stroke reaches, until none is left to merge.
        while let Some(i) = out.iter().position(|c| c.meets(b)) {
            b = b.union(out.swap_remove(i));
        }
        out.push(b);
    }
    let mut out: Vec<BBox> = out.into_iter().map(|b| grow(b, -GAP / 2.0)).collect();
    out.sort_by(|a, b| {
        (a.y0, a.x0)
            .partial_cmp(&(b.y0, b.x0))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    out
}

/// The regions to render: each cluster with a margin, drawn whole at scale 1 when it fits,
/// smaller down to [MIN_SCALE], else tiled into equal pieces at that scale.
pub fn cut(layer: &InkLayer) -> Vec<Cut> {
    let mut out = Vec::new();
    for c in clusters(layer) {
        let r = grow(c, MARGIN);
        let long = (r.x1 - r.x0).max(r.y1 - r.y0);
        if long <= EDGE {
            out.push(Cut {
                rect: r,
                scale: 1.0,
            });
            continue;
        }
        if long * MIN_SCALE <= EDGE {
            out.push(Cut {
                rect: r,
                scale: EDGE / long,
            });
            continue;
        }
        let tile = EDGE / MIN_SCALE;
        let nx = ((r.x1 - r.x0) / tile).ceil().max(1.0) as usize;
        let ny = ((r.y1 - r.y0) / tile).ceil().max(1.0) as usize;
        let (w, h) = ((r.x1 - r.x0) / nx as f32, (r.y1 - r.y0) / ny as f32);
        for j in 0..ny {
            for i in 0..nx {
                let x0 = r.x0 + w * i as f32;
                let y0 = r.y0 + h * j as f32;
                out.push(Cut {
                    rect: BBox {
                        x0,
                        y0,
                        x1: x0 + w,
                        y1: y0 + h,
                    },
                    scale: MIN_SCALE,
                });
            }
        }
    }
    out
}

/// An 8-bit grayscale raster, white to start.
struct Raster {
    w: usize,
    h: usize,
    px: Vec<u8>,
}

impl Raster {
    /// Darken a disc of radius `r` at `(cx, cy)` to `v` (pixel centres inside the disc).
    fn disc(&mut self, cx: f32, cy: f32, r: f32, v: u8) {
        let (x0, x1) = ((cx - r).floor().max(0.0), (cx + r).ceil());
        let (y0, y1) = ((cy - r).floor().max(0.0), (cy + r).ceil());
        let r2 = r * r;
        let mut y = y0;
        while y <= y1 && (y as usize) < self.h {
            let mut x = x0;
            while x <= x1 && (x as usize) < self.w {
                let (dx, dy) = (x + 0.5 - cx, y + 0.5 - cy);
                if dx * dx + dy * dy <= r2 {
                    let p = &mut self.px[y as usize * self.w + x as usize];
                    *p = (*p).min(v);
                }
                x += 1.0;
            }
            y += 1.0;
        }
    }

    /// A stroke as discs stamped along each segment, half a pixel apart.
    fn stroke(&mut self, s: &Stroke, origin: (f32, f32), scale: f32) {
        let v = if s.tool == Tool::Highlighter { 200 } else { 0 };
        let at = |x: f32, y: f32| ((x - origin.0) * scale, (y - origin.1) * scale);
        let radius = |p: f32| (s.width * scale * (0.5 + 0.5 * p) / 2.0).max(0.75);
        let first = s.points[0];
        let (fx, fy) = at(first.x, first.y);
        self.disc(fx, fy, radius(first.pressure), v);
        for w in s.points.windows(2) {
            let (ax, ay) = at(w[0].x, w[0].y);
            let (bx, by) = at(w[1].x, w[1].y);
            let steps = ((bx - ax).hypot(by - ay) * 2.0)
                .ceil()
                .clamp(1.0, 100_000.0) as usize;
            for k in 1..=steps {
                let t = k as f32 / steps as f32;
                let p = w[0].pressure + (w[1].pressure - w[0].pressure) * t;
                self.disc(ax + (bx - ax) * t, ay + (by - ay) * t, radius(p), v);
            }
        }
    }
}

/// Render the strokes meeting `cut` to a PNG (long edge at most [`MAX_REGION_EDGE_PX`]).
pub fn render(layer: &InkLayer, cut: Cut) -> Vec<u8> {
    let r = cut.rect;
    let dim = |d: f32| ((d * cut.scale).round() as usize).clamp(1, MAX_REGION_EDGE_PX as usize);
    let mut ras = Raster {
        w: dim(r.x1 - r.x0),
        h: dim(r.y1 - r.y0),
        px: Vec::new(),
    };
    ras.px = vec![255; ras.w * ras.h];
    for s in layer.strokes_in(r) {
        ras.stroke(s, (r.x0, r.y0), cut.scale);
    }
    super::png::gray8(ras.w, ras.h, &ras.px)
}

/// `rect` as `notes-text.json`'s `[x0, y0, x1, y1]`.
pub fn rect_of(b: BBox) -> Rect {
    [
        f64::from(b.x0),
        f64::from(b.y0),
        f64::from(b.x1),
        f64::from(b.y1),
    ]
}

/// Every region rendered (F17-FR9), and every stroke's box and time for mapping lines back
/// (F19-FR6); a stroke written after `recognised_at_ms` (epoch) is `new` (F19-FR7).
pub fn input(layer: &InkLayer, recognised_at_ms: Option<u64>) -> crate::notes_text::NotesInput {
    input_on(layer, recognised_at_ms, 1)
}

/// [`input`] for canvas `canvas` (F21-FR15: regions are cut from strokes only, on every canvas,
/// with the region's canvas recorded).
pub fn input_on(
    layer: &InkLayer,
    recognised_at_ms: Option<u64>,
    canvas: u32,
) -> crate::notes_text::NotesInput {
    let regions = cut(layer)
        .into_iter()
        .map(|c| RegionImage {
            rect: rect_of(c.rect),
            png: render(layer, c),
            canvas,
        })
        .collect();
    crate::notes_text::NotesInput {
        regions,
        strokes: boxes_on(layer, recognised_at_ms, canvas),
        typed: Vec::new(),
    }
}

/// The strokes as F19's boxes: world bounds, recording time, and whether written since.
pub fn boxes(layer: &InkLayer, recognised_at_ms: Option<u64>) -> Vec<StrokeBox> {
    boxes_on(layer, recognised_at_ms, 1)
}

/// [`boxes`] of canvas `canvas`.
pub fn boxes_on(layer: &InkLayer, recognised_at_ms: Option<u64>, canvas: u32) -> Vec<StrokeBox> {
    layer
        .strokes()
        .iter()
        .map(|s| StrokeBox {
            id: s.id.0,
            bbox: rect_of(s.bounds()),
            at_ms: s.rec_ms,
            new: recognised_at_ms.is_none_or(|t| s.created_at_ms > t),
            canvas,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ink::{InkColor, InkPoint};

    fn line(l: &mut InkLayer, from: (f32, f32), to: (f32, f32), tool: Tool, created: u64) {
        let p = |x, y| InkPoint::new(x, y, 1.0, None, None, 0).unwrap();
        l.add(
            tool,
            InkColor::BLACK,
            4.0,
            vec![p(from.0, from.1), p(to.0, to.1)],
            created,
            Some(created),
        )
        .unwrap();
    }

    /// Two words side by side and a note far below: two regions, the top one first.
    #[test]
    fn nearby_strokes_share_a_region_in_reading_order() {
        let mut l = InkLayer::new();
        line(&mut l, (0.0, 2_000.0), (100.0, 2_000.0), Tool::Pen, 1);
        line(&mut l, (0.0, 0.0), (100.0, 0.0), Tool::Pen, 2);
        line(&mut l, (150.0, 10.0), (250.0, 10.0), Tool::Pen, 3); // 50 from the last: joined
        let c = clusters(&l);
        assert_eq!(c.len(), 2);
        assert_eq!((c[0].x0, c[0].x1, c[0].y0), (-2.0, 252.0, -2.0));
        assert_eq!(c[1].y0, 1_998.0);
        let cuts = cut(&l);
        assert_eq!(cuts[0].scale, 1.0);
        assert_eq!(cuts[0].rect.x0, -26.0);
        assert!(clusters(&InkLayer::new()).is_empty());
    }

    #[test]
    fn a_stroke_bridging_two_clusters_merges_them() {
        let mut l = InkLayer::new();
        line(&mut l, (0.0, 0.0), (10.0, 0.0), Tool::Pen, 1);
        line(&mut l, (500.0, 0.0), (510.0, 0.0), Tool::Pen, 2);
        assert_eq!(clusters(&l).len(), 2);
        line(&mut l, (20.0, 0.0), (490.0, 0.0), Tool::Pen, 3);
        assert_eq!(clusters(&l).len(), 1);
    }

    #[test]
    fn large_ink_is_scaled_then_tiled() {
        let mut l = InkLayer::new();
        line(&mut l, (0.0, 0.0), (2_400.0, 0.0), Tool::Pen, 1);
        let c = cut(&l);
        assert_eq!(c.len(), 1);
        assert!(c[0].scale < 1.0 && c[0].scale >= MIN_SCALE);
        let png = render(&l, c[0]);
        let (w, h) = crate::app::png_size(&png).unwrap();
        assert_eq!(w, MAX_REGION_EDGE_PX);
        assert!(h < 100);
        let mut big = InkLayer::new();
        line(&mut big, (0.0, 0.0), (10_000.0, 0.0), Tool::Pen, 1);
        let tiles = cut(&big);
        assert_eq!(tiles.len(), 4, "10 048 wide in tiles of at most 3 136");
        assert!(tiles.iter().all(|t| t.scale == MIN_SCALE));
        for t in &tiles {
            let (w, _) = crate::app::png_size(&render(&big, *t)).unwrap();
            assert!(w <= MAX_REGION_EDGE_PX);
        }
    }

    #[test]
    fn rendering_draws_pen_black_and_highlighter_grey() {
        let mut l = InkLayer::new();
        line(&mut l, (0.0, 0.0), (60.0, 0.0), Tool::Pen, 1);
        line(&mut l, (0.0, 30.0), (60.0, 30.0), Tool::Highlighter, 2);
        l.add(
            Tool::Pen,
            InkColor::BLACK,
            4.0,
            vec![InkPoint::new(30.0, 60.0, 0.0, None, None, 0).unwrap()],
            3,
            None,
        )
        .unwrap();
        let c = cut(&l);
        assert_eq!(c.len(), 1);
        let r = c[0].rect;
        let mut ras = Raster {
            w: (r.x1 - r.x0) as usize,
            h: (r.y1 - r.y0) as usize,
            px: Vec::new(),
        };
        ras.px = vec![255; ras.w * ras.h];
        for s in l.strokes() {
            ras.stroke(s, (r.x0, r.y0), 1.0);
        }
        let at = |x: f32, y: f32| ras.px[((y - r.y0) as usize) * ras.w + (x - r.x0) as usize];
        assert_eq!(at(30.0, 0.0), 0, "pen");
        assert_eq!(at(30.0, 30.0), 200, "highlighter");
        assert_eq!(at(30.0, 60.0), 0, "a dot");
        assert_eq!(at(30.0, 15.0), 255, "paper");
        assert_eq!(&render(&l, c[0])[..8], b"\x89PNG\r\n\x1a\n");
    }

    #[test]
    fn strokes_written_since_recognition_are_new() {
        let mut l = InkLayer::new();
        line(&mut l, (0.0, 0.0), (10.0, 0.0), Tool::Pen, 1_000);
        line(&mut l, (0.0, 50.0), (10.0, 50.0), Tool::Pen, 3_000);
        let b = boxes(&l, Some(2_000));
        assert_eq!((b[0].new, b[1].new), (false, true));
        assert_eq!(b[1].at_ms, Some(3_000));
        assert_eq!(b[0].bbox, [-2.0, -2.0, 12.0, 2.0]);
        assert!(boxes(&l, None).iter().all(|s| s.new));
        let i = input(&l, None);
        assert_eq!((i.regions.len(), i.strokes.len()), (1, 2));
        assert_eq!(i.regions[0].rect, [-26.0, -26.0, 36.0, 76.0]);
    }
}
