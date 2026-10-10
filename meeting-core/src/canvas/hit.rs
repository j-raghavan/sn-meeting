// Adapted from sn-canvas `android/app/src/main/java/com/sncanvas/canvas/CanvasCore.kt`
// (`hitTest`, `hitsAt`, `isHit`), MIT, Copyright (c) 2026 JRaghavan (notice in
// LICENSES-3RDPARTY.md).
//! F21-FR3 / FR4: what a pen point or an eraser sweep touches among the items. Hit-testing
//! uses item outlines: a filled shape's inside counts, an unfilled one's doesn't
//! [Recommendation, as sn-canvas]. Text boxes, sticky notes, tables, images and types this build
//! doesn't know count as filled boxes; lines and arrows by their resolved segment.
use super::geom::{self, Pt};
use super::item::Item;

/// The distance from `p` to the outline of the item's (rotated) box or ellipse; negative
/// inside.
#[must_use]
pub fn outline_distance(it: &Item, p: Pt) -> f64 {
    let l = geom::to_local(it, p);
    if it.kind == "ellipse" {
        let (a, b) = (it.width / 2.0, it.height / 2.0);
        if a <= 0.0 || b <= 0.0 {
            return geom::dist_to_segment(
                l,
                Pt::new(it.x, it.y),
                Pt::new(it.x + it.width, it.y + it.height),
            );
        }
        let c = geom::centre(it);
        let (dx, dy) = ((l.x - c.x) / a, (l.y - c.y) / b);
        return ((dx * dx + dy * dy).sqrt() - 1.0) * a.min(b);
    }
    let (x0, y0, x1, y1) = (it.x, it.y, it.x + it.width, it.y + it.height);
    let ox = (x0 - l.x).max(l.x - x1).max(0.0);
    let oy = (y0 - l.y).max(l.y - y1).max(0.0);
    if ox > 0.0 || oy > 0.0 {
        return ox.hypot(oy);
    }
    -(l.x - x0).min(x1 - l.x).min(l.y - y0).min(y1 - l.y)
}

/// Whether the item counts as filled for hitting (its inside is part of it).
#[must_use]
pub fn solid(it: &Item) -> bool {
    match it.kind.as_str() {
        "rectangle" | "ellipse" => it.style.filled(),
        _ => true,
    }
}

/// Whether the point `p` within `r` touches the item; `ends` are a line's resolved ends.
#[must_use]
pub fn touches(it: &Item, ends: Option<(Pt, Pt)>, p: Pt, r: f64) -> bool {
    let reach = r + f64::from(it.style.stroke_width()) / 2.0;
    if let Some((a, b)) = ends {
        return geom::dist_to_segment(p, a, b) <= reach.max(geom::LINE_HIT.min(r * 2.0));
    }
    let d = outline_distance(it, p);
    d.abs() <= reach || (solid(it) && d <= 0.0)
}

/// Points along an eraser path, never further apart than `r` (a fast sweep's samples land
/// either side of a thin outline; the points between them still take it).
#[must_use]
pub fn densify(path: &[(f32, f32)], r: f32) -> Vec<Pt> {
    let mut out = Vec::new();
    let step = f64::from(r.max(1.0));
    for (i, &(x, y)) in path.iter().enumerate() {
        let p = Pt::new(f64::from(x), f64::from(y));
        if let Some(&(px, py)) = i.checked_sub(1).map(|j| &path[j]) {
            let q = Pt::new(f64::from(px), f64::from(py));
            let len = (p.x - q.x).hypot(p.y - q.y);
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // bounded below
            let n = ((len / step).ceil() as usize).min(10_000);
            for k in 1..n {
                #[allow(clippy::cast_precision_loss)]
                let t = k as f64 / n as f64;
                out.push(Pt::new(q.x + (p.x - q.x) * t, q.y + (p.y - q.y) * t));
            }
        }
        out.push(p);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::canvas::style::Style;

    fn shape(kind: &str, fill: &str) -> Item {
        let mut it = Item::new("a", kind, Style::default().with("fill", fill));
        (it.x, it.y, it.width, it.height) = (0.0, 0.0, 100.0, 50.0);
        it
    }

    #[test]
    fn an_unfilled_shape_is_its_outline_and_a_filled_one_its_inside_too() {
        let open = shape("rectangle", "none");
        let full = shape("rectangle", "solid");
        let mid = Pt::new(50.0, 25.0);
        assert!(!touches(&open, None, mid, 4.0));
        assert!(touches(&full, None, mid, 4.0));
        assert!(
            touches(&open, None, Pt::new(50.0, 2.0), 4.0),
            "near the top edge"
        );
        assert!(
            touches(&open, None, Pt::new(103.0, 25.0), 4.0),
            "just outside"
        );
        assert!(!touches(&full, None, Pt::new(120.0, 25.0), 4.0));
        assert!((outline_distance(&open, Pt::new(103.0, 54.0)) - 5.0).abs() < 1e-9);
        let e = shape("ellipse", "none");
        assert!(!touches(&e, None, mid, 4.0));
        assert!(touches(&e, None, Pt::new(100.0, 25.0), 4.0));
        assert!(touches(&shape("ellipse", "semi"), None, mid, 4.0));
        let mut flat = shape("ellipse", "none");
        flat.height = 0.0;
        assert!(touches(&flat, None, Pt::new(50.0, 1.0), 4.0));
        for kind in ["text", "note", "table", "image", "sparkle"] {
            assert!(touches(&shape(kind, "none"), None, mid, 4.0), "{kind}");
        }
    }

    #[test]
    fn lines_by_their_segment_and_sweeps_densified() {
        let line = shape("line", "none");
        let ends = Some((Pt::new(0.0, 0.0), Pt::new(100.0, 0.0)));
        assert!(touches(&line, ends, Pt::new(50.0, 5.0), 4.0));
        assert!(!touches(&line, ends, Pt::new(50.0, 30.0), 4.0));
        let d = densify(&[(0.0, 0.0), (10.0, 0.0)], 2.0);
        assert_eq!(d.len(), 6);
        assert_eq!(d.last(), Some(&Pt::new(10.0, 0.0)));
        assert_eq!(densify(&[(1.0, 1.0)], 2.0), vec![Pt::new(1.0, 1.0)]);
    }
}
