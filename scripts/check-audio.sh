#!/bin/bash
set -euo pipefail
# F8-FR3: is a pulled recording real audio, noise or silence? Converts it with macOS
# afconvert to 16-bit WAV in a temp dir and prints its duration, peak and RMS (16-bit units).
# Exits 1 for full-scale noise (peak > 26000 and RMS > 2000) or silence (peak < 100).
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
print(f"duration {frames / rate:.2f} s  peak {peak}  rms {rms:.0f}")
if peak < 100:
    print("silence: no audio in this file")
    sys.exit(1)
if peak > 26000 and rms > 2000:
    print("full-scale noise: not a recording of speech")
    sys.exit(1)
print("looks like real audio")
PY
