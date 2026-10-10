// Adapted from sn-canvas `android/app/src/main/java/com/sncanvas/canvas/CanvasCore.kt`,
// `ShapeEdits.kt` and `ViewTransforms.kt`, MIT, Copyright (c) 2026 JRaghavan (notice in
// LICENSES-3RDPARTY.md).
//! F21: the scene's geometry, ported with sn-canvas's rules and constants: rotated boxes,
//! bound arrow ends projected onto the shape they point at (`CanvasCore.kt:86-104`), hit-tests
//! and the boxes a selection rectangle touches.
use super::item::Item;
use crate::ink::BBox;

/// World-space tolerance for a tap on a line or arrow (`CanvasCore.kt:23`).
pub const LINE_HIT: f64 = 20.0;

/// A world point.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pt {
    pub x: f64,
    pub y: f64,
}

impl Pt {
    #[must_use]
    pub fn new(x: f64, y: f64) -> Pt {
        Pt { x, y }
    }
}

/// A box's centre.
#[must_use]
pub fn centre(it: &Item) -> Pt {
    Pt::new(it.x + it.width / 2.0, it.y + it.height / 2.0)
}

fn rotate_about(c: Pt, p: Pt, angle: f64) -> Pt {
    if angle == 0.0 {
        return p;
    }
    let (s, co) = angle.sin_cos();
    let (dx, dy) = (p.x - c.x, p.y - c.y);
    Pt::new(c.x + dx * co - dy * s, c.y + dx * s + dy * co)
}

/// A world point into the item's unrotated frame.
#[must_use]
pub fn to_local(it: &Item, p: Pt) -> Pt {
    rotate_about(centre(it), p, -it.rotation)
}

/// An unrotated-frame point into world space.
#[must_use]
pub fn to_world(it: &Item, p: Pt) -> Pt {
    rotate_about(centre(it), p, it.rotation)
}

/// The four corners in world space, rotated with the item: top-left, top-right, bottom-left,
/// bottom-right (`CanvasCore.cornerPoints`).
#[must_use]
pub fn corners(it: &Item) -> [Pt; 4] {
    [
        to_world(it, Pt::new(it.x, it.y)),
        to_world(it, Pt::new(it.x + it.width, it.y)),
        to_world(it, Pt::new(it.x, it.y + it.height)),
        to_world(it, Pt::new(it.x + it.width, it.y + it.height)),
    ]
}

/// Whether the world point is inside the (rotated) box.
#[must_use]
pub fn contains(it: &Item, p: Pt) -> bool {
    let l = to_local(it, p);
    l.x >= it.x && l.x <= it.x + it.width && l.y >= it.y && l.y <= it.y + it.height
}

/// The distance from `p` to the segment `a`–`b`.
#[must_use]
pub fn dist_to_segment(p: Pt, a: Pt, b: Pt) -> f64 {
    let (dx, dy) = (b.x - a.x, b.y - a.y);
    let len2 = dx * dx + dy * dy;
    if len2 == 0.0 {
        return (p.x - a.x).hypot(p.y - a.y);
    }
    let t = (((p.x - a.x) * dx + (p.y - a.y) * dy) / len2).clamp(0.0, 1.0);
    (p.x - (a.x + t * dx)).hypot(p.y - (a.y + t * dy))
}

/// A shape an arrow end can bind to: its box and rotation (an item, or a stroke's bounds).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Shape {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    pub rotation: f64,
}

impl Shape {
    #[must_use]
    pub fn of_item(it: &Item) -> Shape {
        Shape {
            x: it.x,
            y: it.y,
            width: it.width,
            height: it.height,
            rotation: it.rotation,
        }
    }

    #[must_use]
    pub fn of_box(b: BBox) -> Shape {
        Shape {
            x: f64::from(b.x0),
            y: f64::from(b.y0),
            width: f64::from(b.x1 - b.x0),
            height: f64::from(b.y1 - b.y0),
            rotation: 0.0,
        }
    }

    #[must_use]
    pub fn centre(&self) -> Pt {
        Pt::new(self.x + self.width / 2.0, self.y + self.height / 2.0)
    }
}

/// The shape's centre pushed out along the ray toward `towards` to where it crosses the
/// shape's box (`CanvasCore.projectToBoundary`), in the shape's own rotated frame.
#[must_use]
pub fn project_to_boundary(s: &Shape, towards: Pt) -> Pt {
    let c = s.centre();
    let local = rotate_about(c, towards, -s.rotation);
    let (dx, dy) = (local.x - c.x, local.y - c.y);
    if dx == 0.0 && dy == 0.0 {
        return c;
    }
    let sx = if dx != 0.0 {
        s.width / 2.0 / dx.abs()
    } else {
        f64::MAX
    };
    let sy = if dy != 0.0 {
        s.height / 2.0 / dy.abs()
    } else {
        f64::MAX
    };
    let k = sx.min(sy);
    rotate_about(c, Pt::new(c.x + dx * k, c.y + dy * k), s.rotation)
}

/// An arrow's ends now (`resolveArrowEndpoints`): a bound end aims at the other end's naive
/// position and lands on its shape's boundary; a free end is where it was put. `shape_of`
/// finds a bound ref's shape (`None`: treated as free).
#[must_use]
pub fn resolve_ends(start: (Pt, Option<Shape>), end: (Pt, Option<Shape>)) -> (Pt, Pt) {
    let naive_start = start.1.map_or(start.0, |s| s.centre());
    let naive_end = end.1.map_or(end.0, |s| s.centre());
    let a = start
        .1
        .map_or(naive_start, |s| project_to_boundary(&s, naive_end));
    let b = end
        .1
        .map_or(naive_end, |s| project_to_boundary(&s, naive_start));
    (a, b)
}

/// Whether a line or arrow from `a` to `b` is under `p`.
#[must_use]
pub fn near_line(a: Pt, b: Pt, p: Pt, tolerance: f64) -> bool {
    dist_to_segment(p, a, b) <= tolerance
}

/// The axis-aligned bounds of points.
#[must_use]
pub fn bounds_of(points: &[Pt]) -> Option<BBox> {
    let first = points.first()?;
    let mut b = (first.x, first.y, first.x, first.y);
    for p in &points[1..] {
        b = (b.0.min(p.x), b.1.min(p.y), b.2.max(p.x), b.3.max(p.y));
    }
    #[allow(clippy::cast_possible_truncation)] // world coordinates are within f32's range
    Some(BBox {
        x0: b.0 as f32,
        y0: b.1 as f32,
        x1: b.2 as f32,
        y1: b.3 as f32,
    })
}

/// Normalises an angle into (-π, π] and snaps it to the nearest 45° when within 5°
/// (`ShapeEdits.snapRotation`).
#[must_use]
pub fn snap_rotation(angle: f64) -> f64 {
    use std::f64::consts::PI;
    let n = angle.sin().atan2(angle.cos());
    let step = PI / 4.0;
    let nearest = (n / step).round() * step;
    let s = if (n - nearest).abs() <= PI / 36.0 {
        nearest
    } else {
        n
    };
    s.sin().atan2(s.cos())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::canvas::style::Style;
    use std::f64::consts::PI;

    fn rect(x: f64, y: f64, w: f64, h: f64, r: f64) -> Item {
        let mut it = Item::new("r", "rectangle", Style::default());
        (it.x, it.y, it.width, it.height, it.rotation) = (x, y, w, h, r);
        it
    }

    fn close(a: Pt, b: Pt) -> bool {
        (a.x - b.x).abs() < 1e-9 && (a.y - b.y).abs() < 1e-9
    }

    #[test]
    fn rotated_boxes_and_containment() {
        let r = rect(0.0, 0.0, 100.0, 50.0, PI / 2.0);
        let c = corners(&r);
        assert!(close(c[0], Pt::new(75.0, -25.0)), "{:?}", c[0]);
        assert!(contains(&r, Pt::new(50.0, -20.0)), "inside once turned");
        assert!(!contains(&r, Pt::new(95.0, 25.0)));
        let p = Pt::new(12.0, 34.0);
        assert!(close(to_world(&r, to_local(&r, p)), p));
        assert!(close(to_world(&rect(0.0, 0.0, 1.0, 1.0, 0.0), p), p));
    }

    /// sn-canvas ConnectorsTest: an end bound to a box lands on its edge facing the other end.
    #[test]
    fn bound_ends_land_on_the_boundary() {
        let a = Shape::of_item(&rect(0.0, 0.0, 100.0, 100.0, 0.0));
        let b = Shape::of_item(&rect(300.0, 0.0, 100.0, 100.0, 0.0));
        let (s, e) = resolve_ends((Pt::new(0.0, 0.0), Some(a)), (Pt::new(0.0, 0.0), Some(b)));
        assert!(close(s, Pt::new(100.0, 50.0)), "{s:?}");
        assert!(close(e, Pt::new(300.0, 50.0)), "{e:?}");
        let free = Pt::new(50.0, 400.0);
        let (s, e) = resolve_ends((Pt::new(0.0, 0.0), Some(a)), (free, None));
        assert!(close(s, Pt::new(50.0, 100.0)));
        assert_eq!(e, free);
        // Pointing at the centre itself stays at the centre.
        assert!(close(
            project_to_boundary(&a, Pt::new(50.0, 50.0)),
            Pt::new(50.0, 50.0)
        ));
        // A vertical ray uses the half-height.
        assert!(close(
            project_to_boundary(&a, Pt::new(50.0, -500.0)),
            Pt::new(50.0, 0.0)
        ));
        let s = Shape::of_box(BBox {
            x0: 0.0,
            y0: 0.0,
            x1: 10.0,
            y1: 20.0,
        });
        assert_eq!(s.centre(), Pt::new(5.0, 10.0));
    }

    #[test]
    fn segments_bounds_and_snapping() {
        let (a, b) = (Pt::new(0.0, 0.0), Pt::new(100.0, 0.0));
        assert_eq!(dist_to_segment(Pt::new(50.0, 7.0), a, b), 7.0);
        assert_eq!(dist_to_segment(Pt::new(-3.0, 4.0), a, b), 5.0);
        assert_eq!(dist_to_segment(Pt::new(3.0, 4.0), a, a), 5.0);
        assert!(near_line(a, b, Pt::new(50.0, 20.0), LINE_HIT));
        assert!(!near_line(a, b, Pt::new(50.0, 21.0), LINE_HIT));
        assert_eq!(bounds_of(&[]), None);
        let bb = bounds_of(&[Pt::new(5.0, -1.0), Pt::new(-2.0, 9.0)]).unwrap();
        assert_eq!((bb.x0, bb.y0, bb.x1, bb.y1), (-2.0, -1.0, 5.0, 9.0));
        assert!((snap_rotation(PI / 4.0 + 0.05) - PI / 4.0).abs() < 1e-12);
        assert!((snap_rotation(0.3) - 0.3).abs() < 1e-12);
        assert!((snap_rotation(2.0 * PI + 0.01)).abs() < 1e-12);
        assert!((snap_rotation(PI) - PI).abs() < 1e-12);
        assert_eq!(centre(&rect(0.0, 0.0, 10.0, 4.0, 0.0)), Pt::new(5.0, 2.0));
    }
}
