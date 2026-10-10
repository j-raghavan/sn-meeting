//! `.inkbin` v2: inkread's versioned little-endian codec (`inkread-ink/src/codec.rs`) for a
//! world-space layer with recording-clock stroke times (OQ14, OQ28 proposed). Decoding is
//! hardened as inkread's: any malformed or truncated input is [`InkError::BadEncoding`], never
//! a panic or an allocation sized by an untrusted count.
//!
//! ```text
//! ["INKB"][ver: u8 = 2][space: u8 = 1 (world)][stroke_count: u32]
//! per stroke:
//!   [id: u32][tool: u8][r u8][g u8][b u8][a u8][width: f32][created_at_ms: u64]
//!   [timed: u8][rec_ms: u64, if timed = 1]
//!   [point_count: u32]
//!   per point: [x: f32][y: f32][pressure: f32][flags: u8][tilt_x: f32?][tilt_y: f32?][ts: u32]
//! ```
//! Differences from v1: the version byte, the space byte, and `timed`/`rec_ms`. `flags` bit 0
//! means a `tilt_x` follows, bit 1 a `tilt_y`. Only strokes persist, never the undo history.
use crate::ink::model::{
    InkColor, InkError, InkLayer, InkPoint, InkResult, Stroke, StrokeId, Tool,
};
use std::collections::HashSet;

const MAGIC: &[u8; 4] = b"INKB";
/// This format's version (inkread's page layers are v1).
pub const INKBIN_VERSION: u8 = 2;
/// The space byte: world coordinates (the only space Meetings writes).
const SPACE_WORLD: u8 = 1;
const FLAG_TILT_X: u8 = 0b01;
const FLAG_TILT_Y: u8 = 0b10;
/// Ceilings on declared counts, as inkread's (a real canvas is far below them).
const MAX_STROKES: u32 = 1_000_000;
const MAX_POINTS: u32 = 10_000_000;

/// Encode the layer's strokes. Pure; never panics.
#[must_use]
pub fn encode_layer(layer: &InkLayer) -> Vec<u8> {
    let strokes = layer.strokes();
    let mut out = Vec::new();
    out.extend_from_slice(MAGIC);
    out.push(INKBIN_VERSION);
    out.push(SPACE_WORLD);
    let count = u32::try_from(strokes.len()).unwrap_or(u32::MAX);
    out.extend_from_slice(&count.to_le_bytes());
    for s in strokes.iter().take(count as usize) {
        out.extend_from_slice(&s.id.0.to_le_bytes());
        out.push(s.tool.code());
        out.extend_from_slice(&[s.color.r, s.color.g, s.color.b, s.color.a]);
        out.extend_from_slice(&s.width.to_le_bytes());
        out.extend_from_slice(&s.created_at_ms.to_le_bytes());
        match s.rec_ms {
            Some(t) => {
                out.push(1);
                out.extend_from_slice(&t.to_le_bytes());
            }
            None => out.push(0),
        }
        let n = u32::try_from(s.points.len()).unwrap_or(u32::MAX);
        out.extend_from_slice(&n.to_le_bytes());
        for p in s.points.iter().take(n as usize) {
            out.extend_from_slice(&p.x.to_le_bytes());
            out.extend_from_slice(&p.y.to_le_bytes());
            out.extend_from_slice(&p.pressure.to_le_bytes());
            let flags = (u8::from(p.tilt_x.is_some()) * FLAG_TILT_X)
                | (u8::from(p.tilt_y.is_some()) * FLAG_TILT_Y);
            out.push(flags);
            for t in [p.tilt_x, p.tilt_y].into_iter().flatten() {
                out.extend_from_slice(&t.to_le_bytes());
            }
            out.extend_from_slice(&p.timestamp_ms.to_le_bytes());
        }
    }
    out
}

fn bad(m: &str) -> InkError {
    InkError::BadEncoding(m.to_owned())
}

/// Decode `.inkbin` v2 bytes. Refuses a bad magic, another version or space, an unknown or
/// eraser tool, a bad width, a stroke without points, a duplicate id, a non-finite point,
/// truncation and trailing bytes.
pub fn decode_layer(bytes: &[u8]) -> InkResult<InkLayer> {
    let mut c = Cursor { buf: bytes, pos: 0 };
    if c.take(4)? != MAGIC {
        return Err(bad("bad magic"));
    }
    let ver = c.u8()?;
    if ver != INKBIN_VERSION {
        return Err(InkError::BadEncoding(format!("unsupported version {ver}")));
    }
    if c.u8()? != SPACE_WORLD {
        return Err(bad("not a world-space layer"));
    }
    let count = c.u32()?;
    if count > MAX_STROKES {
        return Err(bad("stroke count over the limit"));
    }
    let mut strokes = Vec::new();
    let mut seen = HashSet::new();
    for _ in 0..count {
        let id = c.u32()?;
        if !seen.insert(id) {
            return Err(bad("duplicate stroke id"));
        }
        let tool = Tool::from_code(c.u8()?)
            .filter(|t| t.is_ink())
            .ok_or_else(|| bad("unknown tool code"))?;
        let color = InkColor::rgba(c.u8()?, c.u8()?, c.u8()?, c.u8()?);
        let width = c.f32()?;
        if !width.is_finite() || width <= 0.0 {
            return Err(bad("non-finite or non-positive width"));
        }
        let created_at_ms = c.u64()?;
        let rec_ms = match c.u8()? {
            0 => None,
            1 => Some(c.u64()?),
            _ => return Err(bad("bad timed flag")),
        };
        let n = c.u32()?;
        if n == 0 {
            return Err(bad("stroke has no points"));
        }
        if n > MAX_POINTS {
            return Err(bad("point count over the limit"));
        }
        let mut points = Vec::new();
        for _ in 0..n {
            let (x, y, pressure) = (c.f32()?, c.f32()?, c.f32()?);
            let flags = c.u8()?;
            let tilt_x = if flags & FLAG_TILT_X != 0 {
                Some(c.f32()?)
            } else {
                None
            };
            let tilt_y = if flags & FLAG_TILT_Y != 0 {
                Some(c.f32()?)
            } else {
                None
            };
            let ts = c.u32()?;
            points.push(InkPoint::new(x, y, pressure, tilt_x, tilt_y, ts)?);
        }
        strokes.push(Stroke {
            id: StrokeId(id),
            tool,
            color,
            width,
            points,
            created_at_ms,
            rec_ms,
        });
    }
    if c.pos != bytes.len() {
        return Err(bad("trailing bytes after last stroke"));
    }
    Ok(InkLayer::from_strokes(strokes))
}

/// A bounds-checked little-endian reader: reading past the end is `BadEncoding`.
struct Cursor<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl<'a> Cursor<'a> {
    fn take(&mut self, n: usize) -> InkResult<&'a [u8]> {
        let end = self
            .pos
            .checked_add(n)
            .filter(|&e| e <= self.buf.len())
            .ok_or_else(|| bad("unexpected end of data"))?;
        let s = &self.buf[self.pos..end];
        self.pos = end;
        Ok(s)
    }

    fn arr<const N: usize>(&mut self) -> InkResult<[u8; N]> {
        let s = self.take(N)?;
        let mut a = [0u8; N];
        a.copy_from_slice(s);
        Ok(a)
    }

    fn u8(&mut self) -> InkResult<u8> {
        Ok(self.arr::<1>()?[0])
    }

    fn u32(&mut self) -> InkResult<u32> {
        Ok(u32::from_le_bytes(self.arr()?))
    }

    fn u64(&mut self) -> InkResult<u64> {
        Ok(u64::from_le_bytes(self.arr()?))
    }

    fn f32(&mut self) -> InkResult<f32> {
        Ok(f32::from_le_bytes(self.arr()?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn layer() -> InkLayer {
        let mut l = InkLayer::new();
        let p = |x, y, t| InkPoint::new(x, y, 0.5, None, None, t).unwrap();
        l.add(
            Tool::Pen,
            InkColor::BLACK,
            3.0,
            vec![p(-5_000.5, 12_000.25, 0), p(-4_990.0, 12_010.0, 16)],
            1_791_000_000_000,
            Some(450_000),
        )
        .unwrap();
        let tilted = InkPoint::new(10.0, 20.0, 1.0, Some(0.25), Some(-0.5), 0).unwrap();
        let half = InkPoint::new(11.0, 21.0, 1.0, None, Some(0.1), 8).unwrap();
        l.add(
            Tool::Highlighter,
            InkColor::rgba(250, 220, 0, 96),
            30.0,
            vec![tilted, half],
            1_791_000_000_500,
            None,
        )
        .unwrap();
        l
    }

    #[test]
    fn round_trip_keeps_world_points_and_recording_times() {
        let l = layer();
        let bytes = encode_layer(&l);
        assert_eq!(&bytes[..6], b"INKB\x02\x01");
        let back = decode_layer(&bytes).unwrap();
        assert_eq!(back.strokes(), l.strokes());
        assert_eq!(back.strokes()[0].rec_ms, Some(450_000));
        assert_eq!(back.strokes()[1].rec_ms, None);
        assert_eq!(back.strokes()[0].points[0].x, -5_000.5);
        let empty = decode_layer(&encode_layer(&InkLayer::new())).unwrap();
        assert!(empty.is_empty());
    }

    #[test]
    fn every_truncation_is_refused_without_panicking() {
        let bytes = encode_layer(&layer());
        for n in 0..bytes.len() {
            assert!(decode_layer(&bytes[..n]).is_err(), "prefix {n}");
        }
        let mut extra = bytes.clone();
        extra.push(0);
        assert!(decode_layer(&extra).is_err());
    }

    /// The first stroke's header starts at 10: id 10..14, tool 14, rgba 15..19, width 19..23,
    /// created 23..31, timed 31, rec 32..40, count 40..44, first point x 44..48.
    #[test]
    fn hostile_fields_are_refused() {
        let good = encode_layer(&layer());
        let with = |at: usize, b: &[u8]| {
            let mut v = good.clone();
            v[at..at + b.len()].copy_from_slice(b);
            decode_layer(&v)
        };
        assert!(with(0, b"NOPE").is_err());
        assert!(with(4, &[1]).is_err(), "v1 is inkread's page format");
        assert!(with(5, &[0]).is_err(), "page space");
        assert!(
            with(6, &u32::MAX.to_le_bytes()).is_err(),
            "absurd stroke count"
        );
        assert!(with(14, &[2]).is_err(), "an eraser stroke");
        assert!(with(14, &[9]).is_err(), "unknown tool");
        assert!(with(19, &f32::NAN.to_le_bytes()).is_err());
        assert!(with(19, &(-1.0f32).to_le_bytes()).is_err());
        assert!(with(31, &[7]).is_err(), "bad timed flag");
        assert!(with(40, &0u32.to_le_bytes()).is_err(), "no points");
        assert!(
            with(40, &u32::MAX.to_le_bytes()).is_err(),
            "absurd point count"
        );
        assert!(with(44, &f32::INFINITY.to_le_bytes()).is_err());
        // The second stroke reuses id 0.
        let mut l = layer();
        let mut s = l.strokes().to_vec();
        s[1].id = StrokeId(0);
        l = InkLayer::from_strokes(s);
        assert!(decode_layer(&encode_layer(&l)).is_err(), "duplicate id");
    }
}
