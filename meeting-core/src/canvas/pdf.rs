// Adapted from sn-canvas `android/app/src/main/java/com/sncanvas/canvas/PdfPages.kt`, MIT,
// Copyright (c) 2026 JRaghavan (notice in LICENSES-3RDPARTY.md).
//! F21-FR14: the page each canvas exports on: fitted to its content with a 48-unit margin, a
//! world unit printing at 0.75 pt (a screen pixel's size at 96 to the inch), no side over
//! 14 400 pt (content too big is scaled down); an empty canvas gives a blank A4 page.
use crate::ink::BBox;

pub const MARGIN: f64 = 48.0;
pub const POINTS_PER_UNIT: f64 = 0.75;
pub const MAX_SIDE_PT: f64 = 14_400.0;
pub const A4: (u32, u32) = (595, 842);

/// A page and the transform its content is drawn through: `screen = (world - (x, y)) * scale`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Page {
    pub width_pt: u32,
    pub height_pt: u32,
    pub x: f64,
    pub y: f64,
    pub scale: f64,
}

/// The page for content within `bounds` (`PdfPages.fitted`).
#[must_use]
pub fn fitted(bounds: Option<BBox>) -> Page {
    let Some(b) = bounds else {
        return Page {
            width_pt: A4.0,
            height_pt: A4.1,
            x: 0.0,
            y: 0.0,
            scale: POINTS_PER_UNIT,
        };
    };
    let w = f64::from(b.x1 - b.x0) + 2.0 * MARGIN;
    let h = f64::from(b.y1 - b.y0) + 2.0 * MARGIN;
    let scale = POINTS_PER_UNIT.min(MAX_SIDE_PT / w).min(MAX_SIDE_PT / h);
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // at most 14 400
    let side = |v: f64| (v * scale).ceil() as u32;
    Page {
        width_pt: side(w),
        height_pt: side(h),
        x: f64::from(b.x0) - MARGIN,
        y: f64::from(b.y0) - MARGIN,
        scale,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// sn-canvas PdfPagesTest: fitted with a margin at 0.75 pt a unit; huge content scaled to
    /// 14 400 pt; nothing gives A4.
    #[test]
    fn pages_fit_their_content() {
        let b = BBox {
            x0: 100.0,
            y0: 200.0,
            x1: 500.0,
            y1: 400.0,
        };
        assert_eq!(
            fitted(Some(b)),
            Page {
                width_pt: 372,
                height_pt: 222,
                x: 52.0,
                y: 152.0,
                scale: 0.75
            }
        );
        let huge = BBox {
            x0: 0.0,
            y0: 0.0,
            x1: 100_000.0,
            y1: 10.0,
        };
        let p = fitted(Some(huge));
        assert_eq!(p.width_pt, 14_400);
        assert!(p.scale < 0.75);
        assert_eq!((fitted(None).width_pt, fitted(None).height_pt), (595, 842));
    }
}
