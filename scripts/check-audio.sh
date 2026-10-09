#!/bin/bash
set -euo pipefail
# F8-FR3: is a pulled recording real audio, noise or silence? Converts it with macOS
# afconvert to 16-bit WAV in a temp dir and prints its duration, peak, RMS (16-bit units), the
# share of clipped samples (|x| >= 32000) and of quiet 100 ms windows (window RMS < 300), and
# the median window RMS. Exits 1 for silence (peak < 100) or noise: a continuous loud signal
# with no pauses (under 5% quiet windows and median window RMS > 1500). Loud speech may peak at
# full scale; over 1% clipped samples is only a warning.
# Usage: scripts/check-audio.sh <file.m4a>

if [[ $# -ne 1 || ! -f "$1" ]]; then
    echo "usage: $0 <file.m4a>" >&2
    exit 2
fi
command -v afconvert >/dev/null || { echo "afconvert (macOS) is needed" >&2; exit 2; }
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
afconvert -f WAVE -d LEI16 "$1" "$tmp/audio.wav"
python3 -I - "$tmp/audio.wav" <<'PY'
import array, math, sys, wave
with wave.open(sys.argv[1]) as w:
    rate, frames = w.getframerate(), w.getnframes()
    a = array.array("h", w.readframes(frames))
if sys.byteorder == "big":
    a.byteswap()
peak = max((abs(x) for x in a), default=0)
rms = math.sqrt(sum(x * x for x in a) / len(a)) if a else 0.0
clipped = 100 * sum(1 for x in a if abs(x) >= 32000) / len(a) if a else 0.0
step = max(rate // 10, 1)
windows = sorted(
    math.sqrt(sum(x * x for x in a[i:i + step]) / len(a[i:i + step]))
    for i in range(0, len(a), step)
)
quiet = 100 * sum(1 for r in windows if r < 300) / len(windows) if windows else 100.0
median = windows[len(windows) // 2] if windows else 0.0
print(f"duration {frames / rate:.2f} s  peak {peak}  rms {rms:.0f}  clipped {clipped:.3f}%  "
      f"quiet windows {quiet:.0f}%  median window rms {median:.0f}")
if peak < 100:
    print("silence: no audio in this file")
    sys.exit(1)
if quiet < 5 and median > 1500:
    print("noise: loud with no pauses, not a recording of speech")
    sys.exit(1)
if clipped > 1:
    print(f"warning: {clipped:.1f}% of samples clipped; the mic gain may be too high")
print("looks like real audio")
PY
