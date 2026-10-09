#!/bin/bash
set -euo pipefail
# F8-FR3: scripts/check-audio.sh against generated recordings: a soft tone and loud speech
# (bursts with pauses, a few clipped samples) pass, a heavily clipped one passes with a
# warning, and silence and full-scale noise fail. afconvert can't synthesise, so each WAV is
# written by python and then encoded to AAC .m4a with afconvert, as the recorder's files are.
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
def speech(i, gain):
    # 300 ms bursts of a wobbling voice-band signal, then 200 ms pauses (room hiss). Each burst
    # opens with a 10 ms shout at 8x; `gain` scales the whole burst. Both clip at full scale.
    if i % 8000 >= 4800:
        return random.randint(-60, 60)
    v = 9000 * math.sin(2 * math.pi * 180 * i / rate) * (0.6 + 0.4 * math.sin(2 * math.pi * 3 * i / rate))
    v = (v + random.randint(-1500, 1500)) * gain * (8 if i % 8000 < 160 else 1)
    return int(max(-32768, min(32767, v)))
kinds = {
    # A steady tone is continuous like noise, so it must stay below the loud level to pass.
    "tone": lambda i: int(1800 * math.sin(2 * math.pi * 440 * i / rate)),
    "silence": lambda i: 0,
    "noise": lambda i: random.choice((-32000, 32000)),
    "speech": lambda i: speech(i, 1),
    "clipped": lambda i: speech(i, 4),
}
for name, f in kinds.items():
    with wave.open(f"{sys.argv[1]}/{name}.wav", "wb") as w:
        w.setnchannels(1); w.setsampwidth(2); w.setframerate(rate)
        w.writeframes(b"".join(struct.pack("<h", f(i)) for i in range(n)))
PY

failures=0
for kind in tone silence noise speech clipped; do
    afconvert -f m4af -d aac "$tmp/$kind.wav" "$tmp/$kind.m4a"
    set +e
    out="$("$ROOT/scripts/check-audio.sh" "$tmp/$kind.m4a" 2>&1)"
    code=$?
    set -e
    want=0; [[ $kind == silence || $kind == noise ]] && want=1
    warned=no; [[ "$out" == *"warning"* ]] && warned=yes
    want_warn=no; [[ $kind == clipped ]] && want_warn=yes
    if [[ $code -eq $want && $warned == "$want_warn" && "$out" == *"duration 2."* && "$out" == *"clipped"* ]]; then printf '  ok   %s: %s\n' "$kind" "${out//$'\n'/ | }" >&2
    else printf '  FAIL %s (exit %s, want %s; warning %s, want %s): %s\n' "$kind" "$code" "$want" "$warned" "$want_warn" "$out" >&2; failures=$((failures + 1)); fi
done
"$ROOT/scripts/check-audio.sh" >/dev/null 2>&1 && { echo "  FAIL no argument accepted" >&2; failures=$((failures + 1)); }
[[ $failures -eq 0 ]] || exit 1
