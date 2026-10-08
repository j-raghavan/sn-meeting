#!/bin/bash
set -euo pipefail
# F8-FR3: scripts/check-audio.sh against generated recordings: a tone passes, silence and
# full-scale noise fail. afconvert can't synthesise, so each WAV is written by python and then
# encoded to AAC .m4a with afconvert, as the recorder's files are.
# Usage: scripts/test_check_audio.sh  (run by scripts/gates.sh; macOS only)

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
command -v afconvert >/dev/null || { echo "afconvert (macOS) is needed" >&2; exit 1; }
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

# kind → a 2-second 16-bit mono WAV at 16 kHz.
python3 -I - "$tmp" <<'PY'
import math, random, struct, sys, wave
random.seed(7)
rate, n = 16000, 32000
kinds = {
    "tone": lambda i: int(8000 * math.sin(2 * math.pi * 440 * i / rate)),
    "silence": lambda i: 0,
    "noise": lambda i: random.choice((-32000, 32000)),
}
for name, f in kinds.items():
    with wave.open(f"{sys.argv[1]}/{name}.wav", "wb") as w:
        w.setnchannels(1); w.setsampwidth(2); w.setframerate(rate)
        w.writeframes(b"".join(struct.pack("<h", f(i)) for i in range(n)))
PY

failures=0
for kind in tone silence noise; do
    afconvert -f m4af -d aac "$tmp/$kind.wav" "$tmp/$kind.m4a"
    set +e
    out="$("$ROOT/scripts/check-audio.sh" "$tmp/$kind.m4a" 2>&1)"
    code=$?
    set -e
    want=1; [[ $kind == tone ]] && want=0
    if [[ $code -eq $want && "$out" == *"duration 2."* ]]; then printf '  ok   %s: %s\n' "$kind" "${out//$'\n'/ | }" >&2
    else printf '  FAIL %s (exit %s, want %s): %s\n' "$kind" "$code" "$want" "$out" >&2; failures=$((failures + 1)); fi
done
"$ROOT/scripts/check-audio.sh" >/dev/null 2>&1 && { echo "  FAIL no argument accepted" >&2; failures=$((failures + 1)); }
[[ $failures -eq 0 ]] || exit 1
