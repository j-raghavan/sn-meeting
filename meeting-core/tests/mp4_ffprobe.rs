//! F15-FR1 (D14 revised 2026-10-09): our MP4 writer's files, checked by ffprobe and ffmpeg.
//! Real AAC-LC frames come from ffmpeg's encoder (ADTS, split here into access units); the
//! written `.m4a` must report the sample rate, channels and duration, and decode to the same
//! number of frames. A chunk cut from it and a segment rebuilt from chunks must too.
//! `scripts/gates.sh` sets MEETINGS_REQUIRE_FFMPEG=1, so a missing ffmpeg fails there; elsewhere
//! (CI's Rust job has none) the test says so and passes.
use meeting::mp4::{self, Track, Writer};
use std::path::Path;
use std::process::Command;

fn have(tool: &str) -> bool {
    Command::new(tool)
        .arg("-version")
        .output()
        .is_ok_and(|o| o.status.success())
}

/// ADTS frames → (track, access units without their headers).
fn adts(bytes: &[u8]) -> (Track, Vec<Vec<u8>>) {
    let rates = [
        96000, 88200, 64000, 48000, 44100, 32000, 24000, 22050, 16000, 12000, 11025, 8000,
    ];
    let mut i = 0;
    let mut frames = Vec::new();
    let mut head = None;
    while i + 7 <= bytes.len() {
        assert_eq!(bytes[i], 0xff, "ADTS sync at {i}");
        let protection_absent = bytes[i + 1] & 1 == 1;
        let profile = (bytes[i + 2] >> 6) + 1; // audio object type
        let rate_index = (bytes[i + 2] >> 2) & 0x0f;
        let channels = ((bytes[i + 2] & 1) << 2) | (bytes[i + 3] >> 6);
        let len = (usize::from(bytes[i + 3] & 3) << 11)
            | (usize::from(bytes[i + 4]) << 3)
            | usize::from(bytes[i + 5] >> 5);
        let h = if protection_absent { 7 } else { 9 };
        frames.push(bytes[i + h..i + len].to_vec());
        head.get_or_insert((profile, rate_index, channels));
        i += len;
    }
    let (profile, ri, ch) = head.unwrap();
    let asc = [(profile << 3) | (ri >> 1), ((ri & 1) << 7) | (ch << 3)];
    (
        Track::checked(rates[usize::from(ri)], u16::from(ch), &asc).unwrap(),
        frames,
    )
}

fn probe(path: &Path, entries: &str) -> String {
    let o = Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-count_packets",
            "-select_streams",
            "a:0",
            "-show_entries",
            entries,
            "-of",
            "default=nw=1:nk=1",
        ])
        .arg(path)
        .output()
        .unwrap();
    assert!(
        o.status.success(),
        "ffprobe: {}",
        String::from_utf8_lossy(&o.stderr)
    );
    String::from_utf8(o.stdout).unwrap()
}

/// Decodes the whole file with ffmpeg: Ok(decoded frame count) or the error text.
fn decoded_frames(path: &Path) -> usize {
    let o = Command::new("ffmpeg")
        .args(["-nostdin", "-v", "error", "-i"])
        .arg(path)
        .args(["-f", "null", "-"])
        .output()
        .unwrap();
    assert!(
        o.status.success() && o.stderr.is_empty(),
        "ffmpeg decode: {}",
        String::from_utf8_lossy(&o.stderr)
    );
    let o = Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-count_frames",
            "-select_streams",
            "a:0",
            "-show_entries",
            "stream=nb_read_frames",
            "-of",
            "default=nw=1:nk=1",
        ])
        .arg(path)
        .output()
        .unwrap();
    String::from_utf8(o.stdout).unwrap().trim().parse().unwrap()
}

#[test]
fn ffprobe_reads_what_the_writer_wrote() {
    if !have("ffmpeg") || !have("ffprobe") {
        assert!(
            std::env::var("MEETINGS_REQUIRE_FFMPEG").is_err(),
            "ffmpeg and ffprobe are needed here"
        );
        eprintln!("ffmpeg/ffprobe not installed: the MP4 files are not checked by them here");
        return;
    }
    let d = tempfile::tempdir().unwrap();
    for (rate, kbps) in [(16_000u32, "32k"), (44_100, "96k")] {
        let aac = d.path().join(format!("tone-{rate}.aac"));
        let ok = Command::new("ffmpeg")
            .args(["-nostdin", "-v", "error", "-f", "lavfi", "-i"])
            .arg(format!("sine=frequency=440:sample_rate={rate}:duration=6"))
            .args(["-ac", "1", "-c:a", "aac", "-b:a", kbps, "-f", "adts", "-y"])
            .arg(&aac)
            .status()
            .unwrap();
        assert!(ok.success());
        let (track, frames) = adts(&std::fs::read(&aac).unwrap());
        assert_eq!((track.rate, track.channels), (rate, 1));
        let seg = d.path().join(format!("seg-{rate}.m4a"));
        let mut w = Writer::create(&seg, track.clone(), Some(&d.path().join("j.idx"))).unwrap();
        frames.iter().for_each(|f| w.write(f).unwrap());
        let n = w.finish().unwrap() as usize;
        assert_eq!(n, frames.len());
        let info = probe(
            &seg,
            "stream=codec_name,sample_rate,channels,nb_read_packets",
        );
        let lines: Vec<&str> = info.lines().collect();
        assert_eq!(
            lines,
            ["aac", &rate.to_string(), "1", &n.to_string()],
            "{info}"
        );
        let secs: f64 = probe(&seg, "format=duration").trim().parse().unwrap();
        let want = track.ms(n as u64) as f64 / 1000.0;
        assert!((secs - want).abs() < 0.03, "duration {secs} vs {want}");
        assert_eq!(decoded_frames(&seg), n);
        // A chunk cut from it, and the segment rebuilt from two chunks with an overlap.
        let a = d.path().join("a.m4a");
        let b = d.path().join("b.m4a");
        let half = track.ms(n as u64 / 2);
        let na = mp4::cut(&seg, 0, half, &a).unwrap() as usize;
        mp4::cut(&seg, half - 1000, track.ms(n as u64) + 1, &b).unwrap();
        assert_eq!(decoded_frames(&a), na);
        let rebuilt = d.path().join("rebuilt.m4a");
        let back = mp4::concat(&[(a, 0), (b, 1000)], &rebuilt).unwrap() as usize;
        assert!(back.abs_diff(n) <= 1, "{back} of {n}");
        assert_eq!(decoded_frames(&rebuilt), back);
    }
}
