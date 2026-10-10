//! A minimal PNG writer for note regions (F17-FR9): 8-bit grayscale, no interlace, filter 0,
//! zlib from `flate2` (already linked for calendar feeds). Nothing else is needed to send a
//! region to a provider, so no image crate joins the `.so`.
use flate2::Crc;
use flate2::write::ZlibEncoder;
use std::io::Write;

fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    let len = u32::try_from(data.len()).unwrap_or(u32::MAX);
    out.extend_from_slice(&len.to_be_bytes());
    out.extend_from_slice(kind);
    out.extend_from_slice(data);
    let mut crc = Crc::new();
    crc.update(kind);
    crc.update(data);
    out.extend_from_slice(&crc.sum().to_be_bytes());
}

/// `px` (`w × h`, row-major, one byte per pixel) as a PNG.
pub fn gray8(w: usize, h: usize, px: &[u8]) -> Vec<u8> {
    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut ihdr = Vec::with_capacity(13);
    ihdr.extend_from_slice(&u32::try_from(w).unwrap_or(0).to_be_bytes());
    ihdr.extend_from_slice(&u32::try_from(h).unwrap_or(0).to_be_bytes());
    ihdr.extend_from_slice(&[8, 0, 0, 0, 0]); // depth 8, grayscale, deflate, filter 0, no interlace
    chunk(&mut out, b"IHDR", &ihdr);
    let mut z = ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    for row in px.chunks(w.max(1)).take(h) {
        // Writing to a Vec cannot fail.
        let _ = z.write_all(&[0]);
        let _ = z.write_all(row);
    }
    let data = z.finish().unwrap_or_default();
    chunk(&mut out, b"IDAT", &data);
    chunk(&mut out, b"IEND", &[]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use flate2::read::ZlibDecoder;
    use std::io::Read;

    #[test]
    fn a_png_with_the_right_header_crcs_and_pixels() {
        let px = [0u8, 255, 128, 7, 9, 200];
        let png = gray8(3, 2, &px);
        assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
        assert_eq!(crate::app::png_size(&png), Some((3, 2)));
        // IHDR's CRC, as zlib's crc32 of "IHDR" + its data (a known value for 3×2 gray8).
        let mut crc = Crc::new();
        crc.update(&png[12..29]);
        assert_eq!(&png[29..33], &crc.sum().to_be_bytes());
        // IDAT inflates to the filter bytes and rows.
        let len = u32::from_be_bytes(png[33..37].try_into().unwrap()) as usize;
        assert_eq!(&png[37..41], b"IDAT");
        let mut raw = Vec::new();
        ZlibDecoder::new(&png[41..41 + len])
            .read_to_end(&mut raw)
            .unwrap();
        assert_eq!(raw, [0, 0, 255, 128, 0, 7, 9, 200]);
        assert!(png.ends_with(&[0, 0, 0, 0, b'I', b'E', b'N', b'D', 0xAE, 0x42, 0x60, 0x82]));
    }
}
