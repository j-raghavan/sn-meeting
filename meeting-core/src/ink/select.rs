//! Lasso selection (adapted from `inkread-ink/src/select.rs`, ADR-INKREAD-0010): the loop
//! takes whole strokes it encloses or crosses (inkread's "Smart" mode, the only one Meetings
//! offers). Coordinates are world space; the loop is closed last→first.
use crate::ink::model::{BBox, InkLayer, InkPoint, StrokeId};

/// The strokes the loop `polygon` selects, in paint order. Under 3 vertices selects nothing.
#[must_use]
pub fn select_in_polygon(layer: &InkLayer, polygon: &[(f32, f32)]) -> Vec<StrokeId> {
    if polygon.len() < 3 {
        return Vec::new();
    }
    layer
        .strokes()
        .iter()
        .filter(|s| {
            s.points.iter().any(|p| point_in_polygon(p.x, p.y, polygon))
                || crosses_polygon(&s.points, polygon)
        })
        .map(|s| s.id)
        .collect()
}

/// The union of the selected strokes' bounds: the outline drawn round a selection.
#[must_use]
pub fn selection_bounds(layer: &InkLayer, ids: &[StrokeId]) -> Option<BBox> {
    layer.bounds_of(ids)
}

/// Ray-casting point-in-polygon; a point exactly on an edge may go either way.
pub(crate) fn point_in_polygon(px: f32, py: f32, poly: &[(f32, f32)]) -> bool {
    let mut inside = false;
    let mut j = poly.len() - 1;
    for i in 0..poly.len() {
        let (xi, yi) = poly[i];
        let (xj, yj) = poly[j];
        if (yi > py) != (yj > py) {
            let t = (py - yi) / (yj - yi);
            if px < xi + t * (xj - xi) {
                inside = !inside;
            }
        }
        j = i;
    }
    inside
}

/// Whether a segment of the stroke crosses an edge of the closed polygon.
fn crosses_polygon(points: &[InkPoint], poly: &[(f32, f32)]) -> bool {
    points.windows(2).any(|w| {
        let mut j = poly.len() - 1;
        (0..poly.len()).any(|i| {
            let hit = segments_intersect((w[0].x, w[0].y), (w[1].x, w[1].y), poly[j], poly[i]);
            j = i;
            hit
        })
    })
}

/// A proper crossing of `p1`→`p2` and `p3`→`p4` (collinear overlaps don't count).
pub(crate) fn segments_intersect(
    p1: (f32, f32),
    p2: (f32, f32),
    p3: (f32, f32),
    p4: (f32, f32),
) -> bool {
    let d1 = cross(p3, p4, p1);
    let d2 = cross(p3, p4, p2);
    let d3 = cross(p1, p2, p3);
    let d4 = cross(p1, p2, p4);
    ((d1 > 0.0) != (d2 > 0.0)) && ((d3 > 0.0) != (d4 > 0.0))
}

/// The orientation of `c` relative to the line `a`→`b`.
fn cross(a: (f32, f32), b: (f32, f32), c: (f32, f32)) -> f32 {
    (b.0 - a.0) * (c.1 - a.1) - (b.1 - a.1) * (c.0 - a.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ink::model::{InkColor, Tool};

    fn stroke(l: &mut InkLayer, pts: &[(f32, f32)]) -> StrokeId {
        let points = pts
            .iter()
            .map(|&(x, y)| InkPoint::new(x, y, 1.0, None, None, 0).unwrap())
            .collect();
        l.add(Tool::Pen, InkColor::BLACK, 2.0, points, 0, None)
            .unwrap()
    }

    /// A loop round (3000, 3000)–(7000, 7000), far from the origin.
    fn square() -> Vec<(f32, f32)> {
        vec![
            (3_000.0, 3_000.0),
            (7_000.0, 3_000.0),
            (7_000.0, 7_000.0),
            (3_000.0, 7_000.0),
        ]
    }

    #[test]
    fn takes_enclosed_and_crossing_strokes_only() {
        let mut l = InkLayer::new();
        let inside = stroke(&mut l, &[(4_000.0, 4_000.0), (6_000.0, 6_000.0)]);
        let straddle = stroke(&mut l, &[(5_000.0, 5_000.0), (9_000.0, 9_000.0)]);
        let crossing = stroke(&mut l, &[(1_000.0, 5_000.0), (9_000.0, 5_000.0)]);
        let _out = stroke(&mut l, &[(100.0, 100.0), (200.0, 200.0)]);
        assert_eq!(
            select_in_polygon(&l, &square()),
            vec![inside, straddle, crossing]
        );
        assert!(select_in_polygon(&l, &square()[..2]).is_empty());
        let b = selection_bounds(&l, &[inside]).unwrap();
        assert_eq!((b.x0, b.y1), (3_999.0, 6_001.0));
        assert!(selection_bounds(&l, &[]).is_none());
    }

    #[test]
    fn collinear_touching_segments_do_not_cross() {
        assert!(!segments_intersect(
            (0.0, 0.0),
            (2.0, 0.0),
            (1.0, 0.0),
            (3.0, 0.0)
        ));
        assert!(segments_intersect(
            (0.0, -1.0),
            (0.0, 1.0),
            (-1.0, 0.0),
            (1.0, 0.0)
        ));
    }
}
