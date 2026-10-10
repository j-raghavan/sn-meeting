//! F15-FR1 (ADR D14, revised after the device spike 2026-10-09): the near-live recorder's own
//! MP4 (`.m4a`, AAC-LC) writer. On the Manta's firmware `MediaMuxer.stop()` aborts any app
//! process (its MPEG4Writer thread asks for SCHED_FIFO 99, is refused, and later joins the
//! unset thread handle), so the shell's AAC encoder output is written here instead.
//!
//! Layout: `ftyp`, then `mdat` (64-bit size, patched at finish) with every access unit in
//! order, then `moov` with one AAC track whose samples are all in one chunk: `stts` is one run
//! of 1 024-sample frames, `stsz` lists each frame's size, `stco` (or `co64`) the chunk's
//! offset, timescale = the sample rate. Because the index comes last, a file cut short by a
//! crash still holds its frames; the writer also appends each frame's size to a small journal
//! beside the chunks, from which [`repair`] writes the missing index.
//!
//! [`read`] parses files of this shape (and any MP4 with one audio track in stco/co64 chunks),
//! so chunk files can be re-cut from a segment ([`cut`]) and a segment rebuilt from its chunks
//! ([`concat`], F15-FR8) without MediaExtractor or MediaMuxer.
use std::fs::{File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

/// PCM samples per AAC-LC frame.
pub const SAMPLES_PER_FRAME: u32 = 1024;
/// A file bigger than this is not one of ours (a 15-minute segment is about 11 MB).
pub const MAX_FILE_BYTES: u64 = 1 << 31;
/// An access unit bigger than this is not AAC from our encoder.
pub const MAX_FRAME_BYTES: usize = 64 * 1024;

/// The one audio track: sample rate, channels and the AudioSpecificConfig (the codec's
/// `csd-0`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Track {
    pub rate: u32,
    pub channels: u16,
    pub asc: Vec<u8>,
}

impl Track {
    /// A track with a plausible rate, mono or stereo, and a 2–64 byte AudioSpecificConfig.
    pub fn checked(rate: u32, channels: u16, asc: &[u8]) -> io::Result<Track> {
        if !(8_000..=96_000).contains(&rate)
            || !(1..=2).contains(&channels)
            || !(2..=64).contains(&asc.len())
        {
            return Err(invalid("not an AAC track this writer takes"));
        }
        Ok(Track {
            rate,
            channels,
            asc: asc.to_vec(),
        })
    }

    /// Recording ms of `frames` frames.
    pub fn ms(&self, frames: u64) -> u64 {
        frames * u64::from(SAMPLES_PER_FRAME) * 1000 / u64::from(self.rate)
    }

    /// The frame starting at or just after `ms`.
    pub fn frame_at(&self, ms: u64) -> u64 {
        (ms * u64::from(self.rate)).div_ceil(u64::from(SAMPLES_PER_FRAME) * 1000)
    }
}

fn invalid(m: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, m.to_owned())
}

// ---- boxes ----

fn put_box(out: &mut Vec<u8>, kind: &[u8; 4], body: &[u8]) {
    out.extend_from_slice(
        &u32::try_from(8 + body.len())
            .unwrap_or(u32::MAX)
            .to_be_bytes(),
    );
    out.extend_from_slice(kind);
    out.extend_from_slice(body);
}

fn full(version: u8, flags: u32, body: &[u8]) -> Vec<u8> {
    let mut v = vec![version];
    v.extend_from_slice(&flags.to_be_bytes()[1..]);
    v.extend_from_slice(body);
    v
}

fn be32(v: &mut Vec<u8>, x: u32) {
    v.extend_from_slice(&x.to_be_bytes());
}

/// `ftyp` (M4A, compatible with isom and mp42).
fn ftyp() -> Vec<u8> {
    let mut b = Vec::new();
    b.extend_from_slice(b"M4A ");
    be32(&mut b, 0);
    for brand in [b"M4A ", b"isom", b"mp42"] {
        b.extend_from_slice(brand);
    }
    let mut out = Vec::new();
    put_box(&mut out, b"ftyp", &b);
    out
}

/// An MPEG-4 descriptor: tag, length (4-byte form), body.
fn descriptor(tag: u8, body: &[u8]) -> Vec<u8> {
    let n = u32::try_from(body.len()).unwrap_or(0);
    let mut v = vec![tag];
    for shift in [21, 14, 7] {
        v.push(0x80 | ((n >> shift) & 0x7f) as u8);
    }
    v.push((n & 0x7f) as u8);
    v.extend_from_slice(body);
    v
}

fn esds(t: &Track, avg_bitrate: u32, max_frame: u32) -> Vec<u8> {
    let mut dcd = vec![0x40, 0x15]; // MPEG-4 audio, audio stream
    dcd.extend_from_slice(&max_frame.max(1).to_be_bytes()[1..]); // bufferSizeDB (24 bits)
    be32(&mut dcd, avg_bitrate);
    be32(&mut dcd, avg_bitrate);
    dcd.extend_from_slice(&descriptor(0x05, &t.asc));
    let mut es = vec![0, 1, 0]; // ES_ID 1, no flags
    es.extend_from_slice(&descriptor(0x04, &dcd));
    es.extend_from_slice(&descriptor(0x06, &[0x02]));
    full(0, 0, &descriptor(0x03, &es))
}

/// The `moov` for `sizes` frames whose data starts at file offset `data_at`.
pub fn moov(t: &Track, sizes: &[u32], data_at: u64) -> Vec<u8> {
    let n = u32::try_from(sizes.len()).unwrap_or(u32::MAX);
    let duration = u64::from(n) * u64::from(SAMPLES_PER_FRAME);
    let dur32 = u32::try_from(duration).unwrap_or(u32::MAX);
    let total: u64 = sizes.iter().map(|s| u64::from(*s)).sum();
    let secs = (duration / u64::from(t.rate)).max(1);
    let avg = u32::try_from(total * 8 / secs).unwrap_or(u32::MAX);
    let max_frame = sizes.iter().copied().max().unwrap_or(0);
    let matrix: [u32; 9] = [0x10000, 0, 0, 0, 0x10000, 0, 0, 0, 0x4000_0000];

    let mut mvhd = Vec::new();
    for x in [0, 0, t.rate, dur32, 0x10000] {
        be32(&mut mvhd, x); // created, modified, timescale, duration, rate 1.0
    }
    mvhd.extend_from_slice(&[1, 0, 0, 0]); // volume 1.0, reserved
    mvhd.extend_from_slice(&[0; 8]);
    matrix.iter().for_each(|m| be32(&mut mvhd, *m));
    mvhd.extend_from_slice(&[0; 24]);
    be32(&mut mvhd, 2); // next track id

    let mut tkhd = Vec::new();
    for x in [0, 0, 1, 0, dur32] {
        be32(&mut tkhd, x); // created, modified, track 1, reserved, duration (movie timescale)
    }
    tkhd.extend_from_slice(&[0; 8]);
    tkhd.extend_from_slice(&[0, 0, 0, 0, 1, 0, 0, 0]); // layer, group, volume 1.0, reserved
    matrix.iter().for_each(|m| be32(&mut tkhd, *m));
    tkhd.extend_from_slice(&[0; 8]); // width, height

    let mut mdhd = Vec::new();
    for x in [0, 0, t.rate, dur32] {
        be32(&mut mdhd, x);
    }
    mdhd.extend_from_slice(&[0x55, 0xc4, 0, 0]); // language "und"

    let mut hdlr = vec![0; 4];
    hdlr.extend_from_slice(b"soun");
    hdlr.extend_from_slice(&[0; 12]);
    hdlr.extend_from_slice(b"SoundHandler\0");

    let mut mp4a = vec![0; 6];
    mp4a.extend_from_slice(&[0, 1]); // data reference 1
    mp4a.extend_from_slice(&[0; 8]);
    mp4a.extend_from_slice(&t.channels.to_be_bytes());
    mp4a.extend_from_slice(&16u16.to_be_bytes());
    mp4a.extend_from_slice(&[0; 4]);
    be32(&mut mp4a, t.rate.min(65_535) << 16);
    put_box(&mut mp4a, b"esds", &esds(t, avg, max_frame));
    let mut stsd = Vec::new();
    be32(&mut stsd, 1);
    put_box(&mut stsd, b"mp4a", &mp4a);

    let mut stts = Vec::new();
    if n == 0 {
        be32(&mut stts, 0);
    } else {
        for x in [1, n, SAMPLES_PER_FRAME] {
            be32(&mut stts, x);
        }
    }
    let mut stsc = Vec::new();
    if n == 0 {
        be32(&mut stsc, 0);
    } else {
        for x in [1, 1, n, 1] {
            be32(&mut stsc, x);
        }
    }
    let mut stsz = Vec::new();
    be32(&mut stsz, 0);
    be32(&mut stsz, n);
    sizes.iter().for_each(|s| be32(&mut stsz, *s));
    let mut stbl = Vec::new();
    put_box(&mut stbl, b"stsd", &full(0, 0, &stsd));
    put_box(&mut stbl, b"stts", &full(0, 0, &stts));
    put_box(&mut stbl, b"stsc", &full(0, 0, &stsc));
    put_box(&mut stbl, b"stsz", &full(0, 0, &stsz));
    let chunks = u32::from(n > 0);
    match u32::try_from(data_at) {
        Ok(off) => {
            let mut stco = Vec::new();
            be32(&mut stco, chunks);
            if n > 0 {
                be32(&mut stco, off);
            }
            put_box(&mut stbl, b"stco", &full(0, 0, &stco));
        }
        Err(_) => {
            let mut co64 = Vec::new();
            be32(&mut co64, chunks);
            co64.extend_from_slice(&data_at.to_be_bytes());
            put_box(&mut stbl, b"co64", &full(0, 0, &co64));
        }
    }

    let mut dref = Vec::new();
    be32(&mut dref, 1);
    put_box(&mut dref, b"url ", &full(0, 1, &[]));
    let mut dinf = Vec::new();
    put_box(&mut dinf, b"dref", &full(0, 0, &dref));
    let mut minf = Vec::new();
    put_box(&mut minf, b"smhd", &full(0, 0, &[0; 4]));
    put_box(&mut minf, b"dinf", &dinf);
    put_box(&mut minf, b"stbl", &stbl);
    let mut mdia = Vec::new();
    put_box(&mut mdia, b"mdhd", &full(0, 0, &mdhd));
    put_box(&mut mdia, b"hdlr", &full(0, 0, &hdlr));
    put_box(&mut mdia, b"minf", &minf);
    let mut trak = Vec::new();
    put_box(&mut trak, b"tkhd", &full(0, 3, &tkhd));
    put_box(&mut trak, b"mdia", &mdia);
    let mut body = Vec::new();
    put_box(&mut body, b"mvhd", &full(0, 0, &mvhd));
    put_box(&mut body, b"trak", &trak);
    let mut out = Vec::new();
    put_box(&mut out, b"moov", &body);
    out
}

/// Where the frames begin in a file this writer made: after `ftyp` and the 16-byte `mdat`
/// header (64-bit size).
fn data_offset() -> u64 {
    ftyp().len() as u64 + 16
}

// ---- the writer ----

/// One `.m4a` being written: frames are appended as they come; [`Writer::finish`] writes the
/// index. Each frame's size also goes to the journal (4 bytes, little-endian) when there is one.
#[derive(Debug)]
pub struct Writer {
    file: File,
    path: PathBuf,
    journal: Option<(File, PathBuf)>,
    track: Track,
    sizes: Vec<u32>,
}

impl Writer {
    /// Create `path` (replacing any file there) and the journal, empty.
    pub fn create(path: &Path, track: Track, journal: Option<&Path>) -> io::Result<Writer> {
        let mut file = File::create(path)?;
        let mut head = ftyp();
        head.extend_from_slice(&1u32.to_be_bytes()); // size 1: a 64-bit size follows
        head.extend_from_slice(b"mdat");
        head.extend_from_slice(&16u64.to_be_bytes());
        file.write_all(&head)?;
        let journal = match journal {
            Some(j) => {
                if let Some(d) = j.parent() {
                    std::fs::create_dir_all(d)?;
                }
                let mut f = File::create(j)?;
                f.write_all(&journal_header(&track))?;
                Some((f, j.to_path_buf()))
            }
            None => None,
        };
        Ok(Writer {
            file,
            path: path.to_path_buf(),
            journal,
            track,
            sizes: Vec::new(),
        })
    }

    /// Append one access unit.
    pub fn write(&mut self, frame: &[u8]) -> io::Result<()> {
        if frame.is_empty() || frame.len() > MAX_FRAME_BYTES {
            return Err(invalid("not an AAC access unit"));
        }
        self.file.write_all(frame)?;
        let n = u32::try_from(frame.len()).unwrap_or(u32::MAX);
        if let Some((j, _)) = &mut self.journal {
            j.write_all(&n.to_le_bytes())?;
        }
        self.sizes.push(n);
        Ok(())
    }

    /// Frames written so far.
    pub fn frames(&self) -> u64 {
        self.sizes.len() as u64
    }

    /// Finalise: patch the `mdat` size, write the index, sync, and drop the journal. Returns
    /// the frame count; a file with no frame is removed and reported as an error.
    pub fn finish(mut self) -> io::Result<u64> {
        if self.sizes.is_empty() {
            drop(self.file);
            let _ = std::fs::remove_file(&self.path);
            if let Some((_, j)) = &self.journal {
                let _ = std::fs::remove_file(j);
            }
            return Err(invalid("no audio was written"));
        }
        close_file(&mut self.file, &self.track, &self.sizes)?;
        if let Some((_, j)) = self.journal.take() {
            let _ = std::fs::remove_file(j);
        }
        Ok(self.sizes.len() as u64)
    }
}

/// Patch `mdat`'s 64-bit size for `sizes` and append the index; then sync.
fn close_file(file: &mut File, t: &Track, sizes: &[u32]) -> io::Result<()> {
    let data: u64 = sizes.iter().map(|s| u64::from(*s)).sum();
    let at = data_offset();
    file.seek(SeekFrom::Start(at - 8))?;
    file.write_all(&(16 + data).to_be_bytes())?;
    file.seek(SeekFrom::Start(at + data))?;
    file.write_all(&moov(t, sizes, at))?;
    file.set_len(at + data + moov(t, sizes, at).len() as u64)?;
    file.sync_all()
}

/// The journal's header: "M4AJ", rate, channels, ASC length and bytes.
fn journal_header(t: &Track) -> Vec<u8> {
    let mut h = b"M4AJ".to_vec();
    h.extend_from_slice(&t.rate.to_le_bytes());
    h.extend_from_slice(&t.channels.to_le_bytes());
    h.push(u8::try_from(t.asc.len()).unwrap_or(0));
    h.extend_from_slice(&t.asc);
    h
}

/// A journal: the track and every frame size it recorded (a torn last entry is dropped).
fn read_journal(path: &Path) -> io::Result<(Track, Vec<u32>)> {
    let b = crate::atomic::read_capped(path, MAX_FILE_BYTES / 64)?;
    if b.len() < 11 || &b[..4] != b"M4AJ" {
        return Err(invalid("not a frame journal"));
    }
    let rate = u32::from_le_bytes([b[4], b[5], b[6], b[7]]);
    let channels = u16::from_le_bytes([b[8], b[9]]);
    let n = usize::from(b[10]);
    let asc = b
        .get(11..11 + n)
        .ok_or_else(|| invalid("journal header cut short"))?;
    let track = Track::checked(rate, channels, asc)?;
    let sizes = b[11 + n..]
        .chunks_exact(4)
        .map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect();
    Ok((track, sizes))
}

/// F15-FR8: a segment whose writer never finished (the process died) gets its index from the
/// journal: as many frames as both the journal and the file hold. Returns the frame count.
pub fn repair(path: &Path, journal: &Path) -> io::Result<u64> {
    let (track, mut sizes) = read_journal(journal)?;
    let mut file = OpenOptions::new().read(true).write(true).open(path)?;
    let len = file.metadata()?.len();
    let at = data_offset();
    let mut have = len.saturating_sub(at);
    let mut keep = 0;
    for s in &sizes {
        if u64::from(*s) > have {
            break;
        }
        have -= u64::from(*s);
        keep += 1;
    }
    sizes.truncate(keep);
    if sizes.is_empty() {
        return Err(invalid("no whole frame to recover"));
    }
    let mut head = vec![0; at as usize];
    file.read_exact(&mut head)?;
    if head[..ftyp().len()] != ftyp()[..] {
        return Err(invalid("not a file this writer made"));
    }
    close_file(&mut file, &track, &sizes)?;
    let _ = std::fs::remove_file(journal);
    Ok(sizes.len() as u64)
}

// ---- reading ----

/// A parsed file: its track and each frame's (offset, size), in order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Parsed {
    pub track: Track,
    pub frames: Vec<(u64, u32)>,
}

/// One box: its type and body range within `b`.
fn boxes(b: &[u8]) -> Vec<([u8; 4], std::ops::Range<usize>)> {
    let mut out = Vec::new();
    let mut i = 0usize;
    while i + 8 <= b.len() {
        let size = u32::from_be_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]]) as usize;
        let kind = [b[i + 4], b[i + 5], b[i + 6], b[i + 7]];
        let (head, size) = match size {
            1 if i + 16 <= b.len() => {
                let big = u64::from_be_bytes(b[i + 8..i + 16].try_into().unwrap_or([0; 8]));
                (16, usize::try_from(big).unwrap_or(usize::MAX))
            }
            0 => (8, b.len() - i),
            s => (8, s),
        };
        if size < head || i.saturating_add(size) > b.len() {
            break;
        }
        out.push((kind, i + head..i + size));
        i += size;
    }
    out
}

fn child<'a>(b: &'a [u8], path: &[&[u8; 4]]) -> Option<&'a [u8]> {
    let mut cur = b;
    for k in path {
        let (_, r) = boxes(cur).into_iter().find(|(t, _)| t == *k)?;
        cur = &cur[r];
    }
    Some(cur)
}

fn u32_at(b: &[u8], i: usize) -> Option<u32> {
    Some(u32::from_be_bytes(b.get(i..i + 4)?.try_into().ok()?))
}

/// The ASC inside an `esds` body: the DecoderSpecificInfo (tag 5) descriptor's bytes.
fn asc_of(esds: &[u8]) -> Option<Vec<u8>> {
    let mut i = 4; // version and flags
    while i < esds.len() {
        let tag = esds[i];
        i += 1;
        let mut len = 0usize;
        for _ in 0..4 {
            let c = *esds.get(i)?;
            i += 1;
            len = (len << 7) | usize::from(c & 0x7f);
            if c & 0x80 == 0 {
                break;
            }
        }
        match tag {
            0x03 => i += 3, // ES_ID and flags; its children follow
            0x04 => i += 13,
            0x05 => return esds.get(i..i + len).map(<[u8]>::to_vec),
            _ => i += len,
        }
    }
    None
}

/// Parse `path`: an MP4 whose first track is AAC in `mp4a`, with `stsz` sizes and `stco` or
/// `co64` chunk offsets laid out by `stsc`.
pub fn read(path: &Path) -> io::Result<Parsed> {
    let b = crate::atomic::read_capped(path, MAX_FILE_BYTES)?;
    let stbl = child(&b, &[b"moov", b"trak", b"mdia", b"minf", b"stbl"])
        .ok_or_else(|| invalid("no index"))?;
    let mdhd =
        child(&b, &[b"moov", b"trak", b"mdia", b"mdhd"]).ok_or_else(|| invalid("no mdhd"))?;
    let stsd = child(stbl, &[b"stsd"]).ok_or_else(|| invalid("no stsd"))?;
    let mp4a = stsd
        .get(8..)
        .and_then(|e| child(e, &[b"mp4a"]))
        .ok_or_else(|| invalid("not AAC"))?;
    let channels = u16::from_be_bytes([*mp4a.get(16).unwrap_or(&0), *mp4a.get(17).unwrap_or(&0)]);
    let esds = mp4a
        .get(28..)
        .and_then(|e| child(e, &[b"esds"]))
        .ok_or_else(|| invalid("no esds"))?;
    let rate = if mdhd.first() == Some(&1) {
        u32_at(mdhd, 20)
    } else {
        u32_at(mdhd, 12)
    };
    let track = Track::checked(
        rate.unwrap_or(0),
        channels,
        &asc_of(esds).unwrap_or_default(),
    )?;
    let stsz = child(stbl, &[b"stsz"]).ok_or_else(|| invalid("no stsz"))?;
    let fixed = u32_at(stsz, 4).unwrap_or(0);
    let n = u32_at(stsz, 8).unwrap_or(0) as usize;
    if n > b.len() {
        return Err(invalid("absurd sample count"));
    }
    let sizes: Vec<u32> = (0..n)
        .map(|i| {
            if fixed > 0 {
                Some(fixed)
            } else {
                u32_at(stsz, 12 + 4 * i)
            }
        })
        .collect::<Option<_>>()
        .ok_or_else(|| invalid("stsz cut short"))?;
    let offsets: Vec<u64> = if let Some(stco) = child(stbl, &[b"stco"]) {
        let c = u32_at(stco, 4).unwrap_or(0) as usize;
        (0..c.min(b.len()))
            .map(|i| u32_at(stco, 8 + 4 * i).map(u64::from))
            .collect::<Option<_>>()
    } else if let Some(co64) = child(stbl, &[b"co64"]) {
        let c = u32_at(co64, 4).unwrap_or(0) as usize;
        (0..c.min(b.len()))
            .map(|i| {
                Some(u64::from_be_bytes(
                    co64.get(8 + 8 * i..16 + 8 * i)?.try_into().ok()?,
                ))
            })
            .collect::<Option<_>>()
    } else {
        None
    }
    .ok_or_else(|| invalid("no chunk offsets"))?;
    let stsc = child(stbl, &[b"stsc"]).ok_or_else(|| invalid("no stsc"))?;
    let runs: Vec<(u32, u32)> = (0..u32_at(stsc, 4).unwrap_or(0) as usize)
        .map(|i| Some((u32_at(stsc, 8 + 12 * i)?, u32_at(stsc, 12 + 12 * i)?)))
        .collect::<Option<_>>()
        .ok_or_else(|| invalid("stsc cut short"))?;
    let mut frames = Vec::with_capacity(sizes.len());
    let mut next = sizes.iter();
    for (ci, off) in offsets.iter().enumerate() {
        let chunk = u32::try_from(ci + 1).unwrap_or(u32::MAX);
        let per = runs
            .iter()
            .rev()
            .find(|(first, _)| *first <= chunk)
            .map_or(0, |r| r.1);
        let mut at = *off;
        for _ in 0..per {
            let Some(s) = next.next() else { break };
            if at + u64::from(*s) > b.len() as u64 {
                return Err(invalid("a frame lies past the end of the file"));
            }
            frames.push((at, *s));
            at += u64::from(*s);
        }
    }
    if frames.len() != sizes.len() {
        return Err(invalid("the index doesn't account for every frame"));
    }
    Ok(Parsed { track, frames })
}

fn frame_bytes(file: &mut File, (at, size): (u64, u32)) -> io::Result<Vec<u8>> {
    let mut v = vec![0; size as usize];
    file.seek(SeekFrom::Start(at))?;
    file.read_exact(&mut v)?;
    Ok(v)
}

/// Write `out` atomically (through `out.tmp`) from `pieces`: each a file and the frames of it
/// to take. Returns the frame count; nothing is written when there is no frame.
fn write_from(pieces: &[(PathBuf, std::ops::Range<usize>)], out: &Path) -> io::Result<u64> {
    let tmp = crate::atomic::tmp_path(out);
    let mut w: Option<Writer> = None;
    let r = (|| {
        for (p, range) in pieces {
            let parsed = read(p)?;
            let mut f = File::open(p)?;
            let take = range.start.min(parsed.frames.len())..range.end.min(parsed.frames.len());
            for fr in &parsed.frames[take] {
                if w.is_none() {
                    w = Some(Writer::create(&tmp, parsed.track.clone(), None)?);
                }
                if let Some(w) = &mut w {
                    if w.track != parsed.track {
                        return Err(invalid("the pieces are not one track"));
                    }
                    w.write(&frame_bytes(&mut f, *fr)?)?;
                }
            }
        }
        let Some(w) = w.take() else {
            return Err(invalid("no frame to write"));
        };
        let n = w.finish()?;
        std::fs::rename(&tmp, out)?;
        Ok(n)
    })();
    if r.is_err() {
        drop(w);
        let _ = std::fs::remove_file(&tmp);
    }
    r
}

/// F15-FR9 / FR10: chunk `out` re-cut from `segment`'s frames in `[from_ms, to_ms)` (segment
/// time), starting at 0. Returns the frame count.
pub fn cut(segment: &Path, from_ms: u64, to_ms: u64, out: &Path) -> io::Result<u64> {
    let t = read(segment)?.track;
    let (a, b) = (t.frame_at(from_ms) as usize, t.frame_at(to_ms) as usize);
    if let Some(d) = out.parent() {
        std::fs::create_dir_all(d)?;
    }
    write_from(&[(segment.to_path_buf(), a..b)], out)
}

/// F15-FR8: a segment rebuilt from its chunk files, each without the `skip_ms` it repeats of
/// the one before. Written over `target` only when complete. Returns the frame count.
pub fn concat(pieces: &[(PathBuf, u64)], target: &Path) -> io::Result<u64> {
    let mut spans = Vec::new();
    for (p, skip_ms) in pieces {
        let t = read(p)?.track;
        spans.push((p.clone(), t.frame_at(*skip_ms) as usize..usize::MAX));
    }
    write_from(&spans, target)
}

// ---- the JNI-shaped surface (strings in, JSON out; no nativeInit needed) ----

/// The writers open now, by handle (the capture thread holds the handles).
static OPEN: std::sync::Mutex<std::collections::BTreeMap<i64, Writer>> =
    std::sync::Mutex::new(std::collections::BTreeMap::new());
static NEXT: std::sync::atomic::AtomicI64 = std::sync::atomic::AtomicI64::new(1);

fn open_writers() -> std::sync::MutexGuard<'static, std::collections::BTreeMap<i64, Writer>> {
    OPEN.lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn err_json(what: &str, e: &io::Error) -> String {
    serde_json::json!({"error": format!("{what}: {e}")}).to_string()
}

/// `nativeMp4Open(path, rateHz, channels, asc, journal)`: a new file (`journal` "" for none).
/// `{handle}`.
pub fn open_json(path: &str, rate: i32, channels: i32, asc: &[u8], journal: &str) -> String {
    let t = Track::checked(
        u32::try_from(rate).unwrap_or(0),
        u16::try_from(channels).unwrap_or(0),
        asc,
    );
    let j = (!journal.is_empty()).then(|| Path::new(journal));
    match t.and_then(|t| Writer::create(Path::new(path), t, j)) {
        Ok(w) => {
            let h = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            open_writers().insert(h, w);
            serde_json::json!({"handle": h}).to_string()
        }
        Err(e) => err_json("open", &e),
    }
}

/// `nativeMp4Write(handle, frame)`: "" when written, else `{error}`.
pub fn write_json(handle: i64, frame: &[u8]) -> String {
    match open_writers().get_mut(&handle) {
        Some(w) => w
            .write(frame)
            .map_or_else(|e| err_json("write", &e), |()| String::new()),
        None => serde_json::json!({"error": "no such file open"}).to_string(),
    }
}

/// `nativeMp4Close(handle)`: the index written. `{frames, durationMs}`.
pub fn close_json(handle: i64) -> String {
    let Some(w) = open_writers().remove(&handle) else {
        return serde_json::json!({"error": "no such file open"}).to_string();
    };
    let t = w.track.clone();
    match w.finish() {
        Ok(n) => serde_json::json!({"frames": n, "durationMs": t.ms(n)}).to_string(),
        Err(e) => err_json("close", &e),
    }
}

/// `nativeMp4Abandon(handle)`: let the file go without its index (a failed recording); its
/// journal stays for [`repair`]. `{ok}`.
pub fn abandon_json(handle: i64) -> String {
    serde_json::json!({"ok": open_writers().remove(&handle).is_some()}).to_string()
}

/// `nativeMp4Cut(segment, fromMs, toMs, out)` (F15-FR9/FR10). `{frames}`.
pub fn cut_json(segment: &str, from_ms: i64, to_ms: i64, out: &str) -> String {
    let (a, b) = (
        u64::try_from(from_ms).unwrap_or(0),
        u64::try_from(to_ms).unwrap_or(0),
    );
    cut(Path::new(segment), a, b, Path::new(out)).map_or_else(
        |e| err_json("cut", &e),
        |n| serde_json::json!({"frames": n}).to_string(),
    )
}

/// `nativeMp4Rebuild(path, journal, piecesJson)` (F15-FR8): a segment whose writer never
/// finished, repaired from its journal when there is one, else rebuilt from its chunks
/// (`[{path, skipMs}]`). `{frames, from: "journal"|"chunks"}`.
pub fn rebuild_json(path: &str, journal: &str, pieces: &str) -> String {
    let target = Path::new(path);
    if !journal.is_empty()
        && let Ok(n) = repair(target, Path::new(journal))
    {
        return serde_json::json!({"frames": n, "from": "journal"}).to_string();
    }
    let list: Vec<serde_json::Value> = serde_json::from_str(pieces).unwrap_or_default();
    let pieces: Vec<(PathBuf, u64)> = list
        .iter()
        .filter_map(|p| {
            Some((
                PathBuf::from(p.get("path")?.as_str()?),
                p.get("skipMs")
                    .and_then(serde_json::Value::as_u64)
                    .unwrap_or(0),
            ))
        })
        .collect();
    concat(&pieces, target).map_or_else(
        |e| err_json("rebuild", &e),
        |n| serde_json::json!({"frames": n, "from": "chunks"}).to_string(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 16 kHz mono AAC-LC's AudioSpecificConfig: object 2, rate index 8, 1 channel.
    const ASC: [u8; 2] = [0x14, 0x08];

    fn track() -> Track {
        Track::checked(16_000, 1, &ASC).unwrap()
    }

    fn frame(i: u32) -> Vec<u8> {
        (0..(20 + i % 7)).map(|k| (i * 31 + k) as u8).collect()
    }

    fn written(path: &Path, n: u32, journal: Option<&Path>) -> Vec<Vec<u8>> {
        let mut w = Writer::create(path, track(), journal).unwrap();
        let frames: Vec<Vec<u8>> = (0..n).map(frame).collect();
        frames.iter().for_each(|f| w.write(f).unwrap());
        assert_eq!(w.frames(), u64::from(n));
        assert_eq!(w.finish().unwrap(), u64::from(n));
        frames
    }

    fn frames_of(path: &Path) -> Vec<Vec<u8>> {
        let p = read(path).unwrap();
        let mut f = File::open(path).unwrap();
        p.frames
            .iter()
            .map(|fr| frame_bytes(&mut f, *fr).unwrap())
            .collect()
    }

    #[test]
    fn a_written_file_reads_back_frame_for_frame() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("a.m4a");
        let j = d.path().join("j/a.idx");
        let frames = written(&p, 50, Some(&j));
        assert!(!j.exists(), "the journal goes once the file is finished");
        let parsed = read(&p).unwrap();
        assert_eq!(parsed.track, track());
        assert_eq!(parsed.frames[0].0, data_offset());
        assert_eq!(frames_of(&p), frames);
        let b = std::fs::read(&p).unwrap();
        assert_eq!(&b[4..8], b"ftyp");
        assert_eq!(
            &b[data_offset() as usize - 12..data_offset() as usize - 8],
            b"mdat"
        );
    }

    #[test]
    fn times_and_frames() {
        let t = track();
        assert_eq!(t.ms(150), 9_600);
        assert_eq!(t.frame_at(0), 0);
        assert_eq!(t.frame_at(64), 1);
        assert_eq!(t.frame_at(65), 2);
        assert!(Track::checked(7_000, 1, &ASC).is_err());
        assert!(Track::checked(16_000, 3, &ASC).is_err());
        assert!(Track::checked(16_000, 1, &[1]).is_err());
    }

    #[test]
    fn an_empty_file_is_removed_and_bad_frames_refused() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("e.m4a");
        let j = d.path().join("e.idx");
        let mut w = Writer::create(&p, track(), Some(&j)).unwrap();
        assert!(w.write(&[]).is_err());
        assert!(w.write(&vec![0; MAX_FRAME_BYTES + 1]).is_err());
        assert!(w.finish().is_err());
        assert!(!p.exists() && !j.exists());
        assert!(Writer::create(&d.path().join("no/such/dir.m4a"), track(), None).is_err());
    }

    #[test]
    fn a_crashed_segment_is_repaired_from_its_journal() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("s.m4a");
        let j = d.path().join("s.idx");
        let mut w = Writer::create(&p, track(), Some(&j)).unwrap();
        let frames: Vec<Vec<u8>> = (0..30).map(frame).collect();
        frames.iter().for_each(|f| w.write(f).unwrap());
        drop(w); // the process died: no index
        assert!(read(&p).is_err());
        // The last frame was only half on disk.
        let len = std::fs::metadata(&p).unwrap().len();
        OpenOptions::new()
            .write(true)
            .open(&p)
            .unwrap()
            .set_len(len - 5)
            .unwrap();
        assert_eq!(repair(&p, &j).unwrap(), 29);
        assert_eq!(frames_of(&p), frames[..29].to_vec());
        assert!(!j.exists());
        // Nothing to recover, or not our journal or file.
        let e = d.path().join("empty.m4a");
        let ej = d.path().join("empty.idx");
        drop(Writer::create(&e, track(), Some(&ej)).unwrap());
        assert!(repair(&e, &ej).is_err());
        std::fs::write(&ej, b"junk").unwrap();
        assert!(repair(&e, &ej).is_err());
        std::fs::write(&ej, b"M4AJ\x80\x3e\0\0\x01\0\x05\x14").unwrap();
        assert!(repair(&e, &ej).is_err(), "header cut short");
        let other = d.path().join("other.m4a");
        std::fs::write(&other, vec![7u8; 400]).unwrap();
        let oj = d.path().join("other.idx");
        let mut jb = journal_header(&track());
        jb.extend_from_slice(&20u32.to_le_bytes());
        std::fs::write(&oj, jb).unwrap();
        assert!(repair(&other, &oj).is_err(), "not a file this writer made");
    }

    #[test]
    fn chunks_are_cut_from_a_segment_and_a_segment_rebuilt_from_chunks() {
        let d = tempfile::tempdir().unwrap();
        let seg = d.path().join("seg.m4a");
        let frames = written(&seg, 100, None);
        // 1 024 samples at 16 kHz = 64 ms a frame: [640, 1 920) ms is frames 10..30.
        let c = d.path().join("chunks/c-0001.m4a");
        assert_eq!(cut(&seg, 640, 1_920, &c).unwrap(), 20);
        assert_eq!(frames_of(&c), frames[10..30].to_vec());
        assert!(cut(&seg, 9_000, 9_500, &d.path().join("none.m4a")).is_err());
        assert!(!d.path().join("none.m4a").exists());
        // Rebuild from two chunks, the second repeating 2 frames (128 ms) of the first.
        let a = d.path().join("a.m4a");
        let b = d.path().join("b.m4a");
        cut(&seg, 0, 1_280, &a).unwrap(); // frames 0..20
        cut(&seg, 1_152, 3_200, &b).unwrap(); // frames 18..50
        let out = d.path().join("rebuilt.m4a");
        assert_eq!(
            concat(&[(a.clone(), 0), (b.clone(), 128)], &out).unwrap(),
            50
        );
        assert_eq!(frames_of(&out), frames[..50].to_vec());
        // A piece of another track, or a missing one, writes nothing.
        let other = d.path().join("other.m4a");
        let mut w = Writer::create(
            &other,
            Track::checked(44_100, 1, &[0x12, 0x08]).unwrap(),
            None,
        )
        .unwrap();
        w.write(&[1, 2, 3]).unwrap();
        w.finish().unwrap();
        let out2 = d.path().join("mixed.m4a");
        assert!(concat(&[(a.clone(), 0), (other, 0)], &out2).is_err());
        assert!(!out2.exists() && !crate::atomic::tmp_path(&out2).exists());
        assert!(concat(&[(d.path().join("gone.m4a"), 0)], &out2).is_err());
    }

    #[test]
    fn hostile_files_are_refused() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("x.m4a");
        written(&p, 5, None);
        let good = std::fs::read(&p).unwrap();
        // Truncate inside the index, then point the chunk past the end.
        std::fs::write(&p, &good[..good.len() - 10]).unwrap();
        assert!(read(&p).is_err());
        let mut bad = good.clone();
        let stco = bad.windows(4).position(|w| w == b"stco").unwrap();
        bad[stco + 12..stco + 16].copy_from_slice(&u32::MAX.to_be_bytes());
        std::fs::write(&p, &bad).unwrap();
        assert!(read(&p).is_err());
        let mut bad = good.clone();
        let stsz = bad.windows(4).position(|w| w == b"stsz").unwrap();
        bad[stsz + 12..stsz + 16].copy_from_slice(&u32::MAX.to_be_bytes());
        std::fs::write(&p, &bad).unwrap();
        assert!(read(&p).is_err(), "absurd sample count");
        std::fs::write(&p, b"\0\0\0\x08moov").unwrap();
        assert!(read(&p).is_err());
        assert!(
            boxes(&[0, 0, 0, 4, b'a', b'b', b'c', b'd']).is_empty(),
            "size under the header"
        );
        assert_eq!(
            boxes(&[0, 0, 0, 0, b'f', b'r', b'e', b'e', 9]).len(),
            1,
            "size 0: to the end"
        );
        assert_eq!(asc_of(&[0, 0, 0, 0, 0x09, 0x01, 0xff]), None);
        assert_eq!(
            asc_of(&[0, 0, 0, 0, 0x05, 0x80]),
            None,
            "a length cut short"
        );
    }

    #[test]
    fn a_large_offset_uses_co64_and_parses() {
        let m = moov(&track(), &[10, 20], u64::from(u32::MAX) + 9);
        assert!(m.windows(4).any(|w| w == b"co64"));
        let empty = moov(&track(), &[], 40);
        assert!(empty.windows(4).any(|w| w == b"stco"));
        // A co64 file reads: ftyp, mdat at a small offset described by co64.
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("c.m4a");
        let mut b = ftyp();
        let at = b.len() as u64 + 8;
        put_box(&mut b, b"mdat", &[1, 2, 3, 4, 5]);
        let mut mv = moov(&track(), &[2, 3], u64::from(u32::MAX) + 1);
        let co = mv.windows(4).position(|w| w == b"co64").unwrap();
        mv[co + 12..co + 20].copy_from_slice(&at.to_be_bytes());
        b.extend_from_slice(&mv);
        std::fs::write(&p, &b).unwrap();
        let parsed = read(&p).unwrap();
        assert_eq!(parsed.frames, vec![(at, 2), (at + 2, 3)]);
    }

    #[test]
    fn the_jni_surface_writes_cuts_and_rebuilds() {
        let d = tempfile::tempdir().unwrap();
        let p = |n: &str| d.path().join(n).to_string_lossy().into_owned();
        let v = |s: String| serde_json::from_str::<serde_json::Value>(&s).unwrap();
        assert!(v(open_json(&p("x.m4a"), 7, 1, &ASC, ""))["error"].is_string());
        let h = v(open_json(&p("seg.m4a"), 16_000, 1, &ASC, &p("j/seg.idx")))["handle"]
            .as_i64()
            .unwrap();
        for i in 0..40 {
            assert_eq!(write_json(h, &frame(i)), "");
        }
        assert!(v(write_json(h, &[]))["error"].is_string());
        assert!(v(write_json(-5, &[1]))["error"].is_string());
        assert_eq!(
            v(close_json(h)),
            serde_json::json!({"frames": 40, "durationMs": 2_560})
        );
        assert!(v(close_json(h))["error"].is_string());
        assert_eq!(
            v(cut_json(&p("seg.m4a"), 0, 640, &p("c/c1.m4a")))["frames"],
            10
        );
        assert!(v(cut_json(&p("none.m4a"), 0, 640, &p("c/c2.m4a")))["error"].is_string());
        // A crashed segment: repaired from its journal, else rebuilt from its chunk.
        let h2 = v(open_json(&p("crash.m4a"), 16_000, 1, &ASC, &p("crash.idx")))["handle"]
            .as_i64()
            .unwrap();
        (0..12).for_each(|i| assert_eq!(write_json(h2, &frame(i)), ""));
        assert_eq!(v(abandon_json(h2)), serde_json::json!({"ok": true}));
        assert_eq!(v(abandon_json(h2)), serde_json::json!({"ok": false}));
        assert_eq!(
            v(rebuild_json(&p("crash.m4a"), &p("crash.idx"), "[]")),
            serde_json::json!({"frames": 12, "from": "journal"})
        );
        let pieces = serde_json::json!([{"path": p("c/c1.m4a"), "skipMs": 0}]).to_string();
        assert_eq!(
            v(rebuild_json(&p("lost.m4a"), &p("gone.idx"), &pieces)),
            serde_json::json!({"frames": 10, "from": "chunks"})
        );
        assert!(v(rebuild_json(&p("lost2.m4a"), "", "junk"))["error"].is_string());
        assert!(v(open_json(&p("no/dir/x.m4a"), 16_000, 1, &ASC, ""))["error"].is_string());
    }
}
