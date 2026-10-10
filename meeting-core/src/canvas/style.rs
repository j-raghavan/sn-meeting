// Adapted from sn-canvas `android/app/src/main/java/com/sncanvas/canvas/CanvasStyle.kt`, MIT,
// Copyright (c) 2026 JRaghavan (notice in LICENSES-3RDPARTY.md). The colour values are
// tldraw's published defaults, used as data.
//! F21-FR6: an item's or stroke's look, stored by ids: colour, opacity, fill, outline (dash)
//! and size. An unknown id or value leaves the style as it was (`CanvasStyle.kt:95-111`), "so
//! no UI message can corrupt a canvas".
use serde_json::{Map, Value, json};

/// The 12 colours (tldraw's) by id, with their true RGB (`CanvasStyle.kt:16-36`). The PDF uses
/// these; the screen draws each as a grey of its own (the shell's `StylePalette.EINK`).
pub const COLORS: [(&str, u32); 12] = [
    ("black", 0x1D1D1D),
    ("grey", 0x9FA8B2),
    ("light-violet", 0xE085F4),
    ("violet", 0xAE3EC9),
    ("blue", 0x4465E9),
    ("light-blue", 0x4BA1F1),
    ("yellow", 0xF1AC4B),
    ("orange", 0xE16919),
    ("green", 0x099268),
    ("light-green", 0x4CB05E),
    ("light-red", 0xF87777),
    ("red", 0xE03131),
];

/// Fills (`CanvasStyle.kt:38-50`); `pattern` is the hatched fill.
pub const FILLS: [&str; 5] = ["none", "semi", "solid", "pattern", "gradient"];

/// Outlines (`:52-63`): `draw` is hand-drawn; `none` is for images only.
pub const DASHES: [&str; 5] = ["draw", "dashed", "dotted", "solid", "none"];

/// Sizes: id, stroke width and font size in world units (`:65-75`).
pub const SIZES: [(&str, f32, f32); 4] = [
    ("s", 2.0, 18.0),
    ("m", 3.5, 24.0),
    ("l", 5.0, 36.0),
    ("xl", 10.0, 44.0),
];

/// The lowest opacity a style takes.
pub const MIN_OPACITY: f64 = 0.1;

/// One look. The ids are always known ones; unknown keys of a stored style are kept in
/// `extra` and written back.
#[derive(Clone, Debug, PartialEq)]
pub struct Style {
    pub color: &'static str,
    pub opacity: f64,
    pub fill: &'static str,
    pub dash: &'static str,
    pub size: &'static str,
    pub extra: Map<String, Value>,
}

impl Default for Style {
    /// `{color: black, opacity: 1, fill: none, dash: draw, size: m}` (F21-FR6's default).
    fn default() -> Self {
        Style {
            color: "black",
            opacity: 1.0,
            fill: "none",
            dash: "draw",
            size: "m",
            extra: Map::new(),
        }
    }
}

fn known<const N: usize>(ids: [&'static str; N], v: &str) -> Option<&'static str> {
    ids.into_iter().find(|i| *i == v)
}

fn color_id(v: &str) -> Option<&'static str> {
    COLORS.iter().map(|c| c.0).find(|i| *i == v)
}

fn size_id(v: &str) -> Option<&'static str> {
    SIZES.iter().map(|s| s.0).find(|i| *i == v)
}

impl Style {
    /// This style with one property changed, as the style panel sends it. An unknown property
    /// or value leaves it as it is.
    #[must_use]
    pub fn with(&self, property: &str, value: &str) -> Style {
        let mut s = self.clone();
        match property {
            "color" => s.color = color_id(value).unwrap_or(s.color),
            "fill" => s.fill = known(FILLS, value).unwrap_or(s.fill),
            "dash" => s.dash = known(DASHES, value).unwrap_or(s.dash),
            "size" => s.size = size_id(value).unwrap_or(s.size),
            "opacity" => {
                if let Some(o) = value.parse::<f64>().ok().filter(|o| o.is_finite()) {
                    s.opacity = o.clamp(MIN_OPACITY, 1.0);
                }
            }
            _ => {}
        }
        s
    }

    /// A stored style: each known key with a known value is taken, anything else leaves the
    /// default's; keys this build doesn't know are kept.
    #[must_use]
    pub fn from_value(v: &Value) -> Style {
        let mut s = Style::default();
        let Some(o) = v.as_object() else {
            return s;
        };
        for (k, val) in o {
            match (k.as_str(), val) {
                ("opacity", Value::Number(n)) => s = s.with("opacity", &n.to_string()),
                ("color" | "fill" | "dash" | "size", Value::String(x)) => s = s.with(k, x),
                ("color" | "fill" | "dash" | "size" | "opacity", _) => {}
                _ => {
                    s.extra.insert(k.clone(), val.clone());
                }
            }
        }
        s
    }

    /// The stored form.
    #[must_use]
    pub fn to_value(&self) -> Value {
        let mut o = self.extra.clone();
        for (k, v) in [
            ("color", json!(self.color)),
            ("opacity", json!(self.opacity)),
            ("fill", json!(self.fill)),
            ("dash", json!(self.dash)),
            ("size", json!(self.size)),
        ] {
            o.insert(k.into(), v);
        }
        Value::Object(o)
    }

    /// The stroke width in world units of this size.
    #[must_use]
    pub fn stroke_width(&self) -> f32 {
        SIZES.iter().find(|s| s.0 == self.size).map_or(3.5, |s| s.1)
    }

    /// The font size in world units of this size.
    #[must_use]
    pub fn font_size(&self) -> f32 {
        SIZES
            .iter()
            .find(|s| s.0 == self.size)
            .map_or(24.0, |s| s.2)
    }

    /// The true RGB of this colour, as `0xRRGGBB`.
    #[must_use]
    pub fn rgb(&self) -> u32 {
        COLORS.iter().find(|c| c.0 == self.color).map_or(0, |c| c.1)
    }

    /// Whether the fill paints something (a filled shape's inside counts for hit-testing).
    #[must_use]
    pub fn filled(&self) -> bool {
        self.fill != "none"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_values_leave_the_style_alone() {
        let s = Style::default();
        assert_eq!(s.with("color", "magenta"), s);
        assert_eq!(s.with("shadow", "on"), s);
        assert_eq!(s.with("opacity", "NaN"), s);
        assert_eq!(s.with("opacity", "much"), s);
        let b = s
            .with("color", "blue")
            .with("size", "l")
            .with("fill", "pattern");
        assert_eq!((b.color, b.size, b.fill), ("blue", "l", "pattern"));
        assert_eq!(b.with("dash", "dotted").dash, "dotted");
        assert_eq!(s.with("opacity", "0.01").opacity, MIN_OPACITY);
        assert_eq!(s.with("opacity", "7").opacity, 1.0);
        assert_eq!(s.with("opacity", "0.5").opacity, 0.5);
    }

    #[test]
    fn stored_styles_round_trip_and_keep_unknown_keys() {
        let v = json!({"color": "red", "opacity": 0.4, "fill": "semi", "dash": "dashed",
                       "size": "xl", "glow": 3});
        let s = Style::from_value(&v);
        assert_eq!(
            (s.color, s.opacity, s.fill, s.dash, s.size),
            ("red", 0.4, "semi", "dashed", "xl")
        );
        assert_eq!(s.to_value(), v);
        let odd = Style::from_value(&json!({"color": 7, "size": "huge", "opacity": "x"}));
        assert_eq!(
            Style {
                extra: Map::new(),
                ..odd.clone()
            },
            Style::default()
        );
        assert_eq!(Style::from_value(&json!("blue")), Style::default());
        assert_eq!((s.stroke_width(), s.font_size()), (10.0, 44.0));
        assert_eq!(s.rgb(), 0xE03131);
        assert!(s.filled() && !Style::default().filled());
        let bad = Style {
            size: "?",
            color: "?",
            ..Style::default()
        };
        assert_eq!(
            (bad.stroke_width(), bad.font_size(), bad.rgb()),
            (3.5, 24.0, 0)
        );
    }

    #[test]
    fn the_palette_has_twelve_distinct_colours() {
        let mut ids: Vec<&str> = COLORS.iter().map(|c| c.0).collect();
        ids.dedup();
        assert_eq!(ids.len(), 12);
        let mut rgb: Vec<u32> = COLORS.iter().map(|c| c.1).collect();
        rgb.sort_unstable();
        rgb.dedup();
        assert_eq!(rgb.len(), 12);
    }
}
