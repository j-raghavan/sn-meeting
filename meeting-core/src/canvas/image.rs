// Adapted from sn-canvas `android/app/src/main/java/com/sncanvas/canvas/ImageElements.kt`
// (`fileNameFor`, `create`) and `ImageCache.kt` (`import`), MIT, Copyright (c) 2026 JRaghavan
// (notice in LICENSES-3RDPARTY.md).
//! F21-FR11: images. A picked file is untrusted (Security): the core checks its extension
//! against its first bytes (PNG, JPEG, RIFF-WEBP), its size (≤ 20 MB) and its pixel count
//! (≤ 50 MP), refuses links and `..`, then **copies** it to `<meeting>.notes/images/<itemId>.<ext>`
//! (`jpeg` normalised to `jpg`) and places it centred in the view at no more than 60 % of it.
use super::edits::Made;
use super::item::{Image, Item, Ref};
use super::scene::Scene;
use crate::ink::BBox;
use std::path::Path;

/// The largest file taken [Recommendation].
pub const MAX_IMAGE_BYTES: u64 = 20 * 1024 * 1024;
/// The most pixels taken [Recommendation].
pub const MAX_PIXELS: u64 = 50_000_000;
/// A new image fills at most this share of the view (`ImageElements.VIEW_SHARE`).
pub const VIEW_SHARE: f64 = 0.6;

/// The normalised extension (`png`, `jpg`, `webp`) of a name, if it is one we take.
#[must_use]
pub fn extension(name: &str) -> Option<&'static str> {
    let ext = name.rsplit_once('.')?.1.to_ascii_lowercase();
    match ext.as_str() {
        "png" => Some("png"),
        "jpg" | "jpeg" => Some("jpg"),
        "webp" => Some("webp"),
        _ => None,
    }
}

/// Whether the first bytes are what the extension says.
#[must_use]
pub fn magic_matches(ext: &str, head: &[u8]) -> bool {
    match ext {
        "png" => head.starts_with(b"\x89PNG\r\n\x1a\n"),
        "jpg" => head.starts_with(&[0xFF, 0xD8, 0xFF]),
        "webp" => head.len() >= 12 && &head[0..4] == b"RIFF" && &head[8..12] == b"WEBP",
        _ => false,
    }
}

/// Read and check a picked file; its bytes and normalised extension, or why it is refused.
pub fn read_checked(src: &Path, px_w: u32, px_h: u32) -> Result<(Vec<u8>, &'static str), String> {
    if src
        .components()
        .any(|c| matches!(c, std::path::Component::ParentDir))
    {
        return Err("That file's name isn't allowed".into());
    }
    let name = src.file_name().and_then(|n| n.to_str()).unwrap_or("");
    let ext = extension(name).ok_or("Only PNG, JPG and WebP images can be inserted")?;
    let meta = std::fs::symlink_metadata(src).map_err(|_| "That image can't be read".to_owned())?;
    if !meta.file_type().is_file() {
        return Err("That isn't a plain image file".into());
    }
    if meta.len() > MAX_IMAGE_BYTES {
        return Err("That image is over 20 MB".into());
    }
    if px_w == 0 || px_h == 0 {
        return Err("That image can't be read".into());
    }
    if u64::from(px_w) * u64::from(px_h) > MAX_PIXELS {
        return Err("That image is over 50 megapixels".into());
    }
    let bytes = crate::atomic::read_capped(src, MAX_IMAGE_BYTES)
        .map_err(|_| "That image can't be read".to_owned())?;
    if !magic_matches(ext, &bytes) {
        return Err("That file isn't the image its name says".into());
    }
    Ok((bytes, ext))
}

/// The box a new image takes: centred in `view`, at most 60 % of it, never larger than its own
/// pixels at 100 % (`ImageElements.create`); a view with no size shows it at its own size.
#[must_use]
pub fn placed(view: BBox, w: u32, h: u32) -> (f64, f64, f64, f64) {
    let (vw, vh) = (f64::from(view.x1 - view.x0), f64::from(view.y1 - view.y0));
    let (pw, ph) = (f64::from(w), f64::from(h));
    let fit = (vw * VIEW_SHARE / pw).min(vh * VIEW_SHARE / ph);
    let scale = if fit > 0.0 { fit.min(1.0) } else { 1.0 };
    let (iw, ih) = (pw * scale, ph * scale);
    let cx = f64::from(view.x0 + view.x1) / 2.0;
    let cy = f64::from(view.y0 + view.y1) / 2.0;
    (cx - iw / 2.0, cy - ih / 2.0, iw, ih)
}

impl Scene {
    /// F21-FR11: an image item for a copy already in `images/` as `file`, placed in `view`,
    /// outline none; one step.
    pub fn create_image(
        &mut self,
        id: &str,
        file: &str,
        w: u32,
        h: u32,
        view: BBox,
        made: &Made<'_>,
    ) -> Option<Ref> {
        let mut it = Item::new(id, "image", made.style.with("dash", "none"));
        (it.x, it.y, it.width, it.height) = placed(view, w, h);
        it.image = Some(Image {
            file: file.into(),
            width: w,
            height: h,
        });
        it.rec_ms = made.rec_ms;
        it.created_at = Some(made.created_at.to_owned());
        self.add_item(it)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::canvas::style::Style;
    use crate::ink::InkLayer;

    #[test]
    fn extensions_and_first_bytes() {
        assert_eq!(extension("photo.JPEG"), Some("jpg"));
        assert_eq!(extension("a.png"), Some("png"));
        assert_eq!(extension("a.webp"), Some("webp"));
        assert_eq!(extension("a.gif"), None);
        assert_eq!(extension("noext"), None);
        assert!(magic_matches("png", b"\x89PNG\r\n\x1a\nrest"));
        assert!(magic_matches("jpg", &[0xFF, 0xD8, 0xFF, 0xE0]));
        assert!(magic_matches("webp", b"RIFF\0\0\0\0WEBPVP8 "));
        assert!(!magic_matches("png", b"hello"));
        assert!(!magic_matches("webp", b"RIFF"));
        assert!(!magic_matches("gif", b"GIF89a"));
    }

    #[test]
    fn images_are_placed_in_the_view() {
        let view = BBox {
            x0: 0.0,
            y0: 0.0,
            x1: 1000.0,
            y1: 1000.0,
        };
        assert_eq!(placed(view, 1600, 1200), (200.0, 275.0, 600.0, 450.0));
        assert_eq!(
            placed(view, 100, 50),
            (450.0, 475.0, 100.0, 50.0),
            "never above its own size"
        );
        let none = BBox {
            x0: 5.0,
            y0: 5.0,
            x1: 5.0,
            y1: 5.0,
        };
        assert_eq!(placed(none, 10, 10), (0.0, 0.0, 10.0, 10.0));
        let mut s = Scene::load(InkLayer::new(), Ok(None)).0;
        let made = Made {
            style: Style::default(),
            rec_ms: None,
            created_at: "t",
        };
        let r = s
            .create_image("im", "im.jpg", 1600, 1200, view, &made)
            .unwrap();
        assert_eq!(r, Ref::Item("im".into()));
        assert_eq!(s.item("im").unwrap().style.dash, "none");
    }
}
