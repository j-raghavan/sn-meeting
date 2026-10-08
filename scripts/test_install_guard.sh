#!/bin/bash
set -euo pipefail
# F1-FR7: the buildApk.sh --install recording guard, against a stubbed adb (no device).
# Usage: scripts/test_install_guard.sh  (run by scripts/gates.sh)

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
# shellcheck source=buildApk.sh
source "$ROOT/buildApk.sh"

# The stub: `adb [-s serial] shell dumpsys …` prints $SERVICES / $AUDIO, or fails on request.
SERVICES="" AUDIO="" FAIL=""
adb() {
    # With --device, every call must carry exactly that serial.
    if [[ ${#ADB_DEVICE[@]} -gt 0 ]]; then
        [[ "$1 $2" == "-s 10.0.0.22:5555" ]] || { echo "adb without the serial: $*" >&2; return 3; }
        shift 2
    fi
    case "$*" in
        "shell dumpsys activity services $PKG_ID") [[ "$FAIL" != services ]] || return 1; printf '%s\n' "$SERVICES" ;;
        "shell dumpsys audio") [[ "$FAIL" != audio ]] || return 1; printf '%s\n' "$AUDIO" ;;
        *) echo "unexpected adb call: $*" >&2; return 2 ;;
    esac
}

failures=0
check() { # name, expected, actual
    if [[ "$2" == "$3" ]]; then printf '  ok   %s\n' "$1" >&2
    else printf '  FAIL %s: want %s, got %s\n' "$1" "$2" "$3" >&2; failures=$((failures + 1)); fi
}

IDLE_SERVICES="ACTIVITY MANAGER SERVICES (dumpsys activity services)
  (nothing)"
header="
RecordActivityMonitor dump time: 10:15:02 AM"
ours="  session:73 -- source client=MIC, dev=AUDIO_DEVICE_IN_USB_DEVICE -- uid:10123 -- patch:12 -- pack:dev.jraghavan.snmeeting -- silenced:false"
theirs="${ours/dev.jraghavan.snmeeting/com.ratta.supernote.note}"
lookalike="${ours/dev.jraghavan.snmeeting/dev.jraghavan.snmeeting2}"
events="

Audio event log: recording activity received by AudioService
10-07 21:16:40:012 rec start riid:9 uid:10123 session:73 src:MIC pack:dev.jraghavan.snmeeting
10-07 21:17:11:005 rec stop riid:9 uid:10123 session:73 src:MIC pack:dev.jraghavan.snmeeting"
next_section="
AudioDeviceBroker:
  pack:dev.jraghavan.snmeeting"

state() { SERVICES="$1" AUDIO="$2" FAIL="${3:-}"; recording_state; }

check "RecordingService running"     yes     "$(state "  * ServiceRecord{1 u0 $PKG_ID/.RecordingService}" "")"
check "active riid, our package"     yes     "$(state "$IDLE_SERVICES" "$header
riid 9; active true
$ours$events")"
check "inactive riid, our package"   no      "$(state "$IDLE_SERVICES" "$header
riid 9; active false
$ours$events")"
check "riid -1, our package"         yes     "$(state "$IDLE_SERVICES" "$header
riid -1; active false
$ours$events")"
check "riid -12, our package"        no      "$(state "$IDLE_SERVICES" "$header
riid -12; active false
$ours$events")"
check "no riid lines (older format)" yes     "$(state "$IDLE_SERVICES" "$header
$ours")"
check "only the event log names us"  no      "$(state "$IDLE_SERVICES" "$header$events")"
check "another app recording"        no      "$(state "$IDLE_SERVICES" "$header
riid 3; active true
$theirs")"
check "a look-alike package"         no      "$(state "$IDLE_SERVICES" "$header
riid 3; active true
$lookalike")"
check "a later section names us"     no      "$(state "$IDLE_SERVICES" "$header$next_section")"
check "no RecordActivityMonitor"     unknown "$(state "$IDLE_SERVICES" "Audio dump without the monitor")"
check "dumpsys services fails"       unknown "$(state "" "" services)"
check "dumpsys audio fails"          unknown "$(state "$IDLE_SERVICES" "" audio)"

# confirm_no_recording: idle passes; recording or unknown asks, and only "y" proceeds.
confirm() { ( SERVICES="$1" AUDIO="$2" FAIL="${3:-}"; confirm_no_recording <<< "$4" >/dev/null 2>&1 ) && echo go || echo stop; }
active="$header
riid 9; active true
$ours"
check "idle installs without asking" go   "$(confirm "$IDLE_SERVICES" "$header" "" "")"
check "recording, answer n"          stop "$(confirm "$IDLE_SERVICES" "$active" "" n)"
check "recording, answer y"          go   "$(confirm "$IDLE_SERVICES" "$active" "" y)"
check "recording, no answer"         stop "$(confirm "$IDLE_SERVICES" "$active" "" "")"
check "unknown, answer n"            stop "$(confirm "" "" services n)"
check "unknown, answer y"            go   "$(confirm "" "" services y)"
ADB_DEVICE=(-s 10.0.0.22:5555)
check "--device is passed through"   yes  "$(state "$IDLE_SERVICES" "$active")"

[[ "$failures" == 0 ]] || { printf '\033[31m%s install-guard check(s) failed\033[0m\n' "$failures" >&2; exit 1; }
printf '\033[32mInstall guard: all checks passed\033[0m\n' >&2
