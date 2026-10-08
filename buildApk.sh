#!/bin/bash
set -euo pipefail

# =========================================================
# buildApk.sh — build the Meetings APK
#
# Pipeline:
#   1. gates      → scripts/gates.sh (fmt, clippy, tests, coverage, lint); --no-gates skips it
#   2. cargo-ndk  → build the Rust core (libmeeting.so) into app/src/main/jniLibs, prune any
#                   other .so cargo-ndk copies there, strip with the NDK's llvm-strip
#   3. gradle     → assembleRelease (or assembleDebug)
#   4. sizes      → libmeeting.so and APK bytes, and the .so's entry in the APK (F1-FR10)
#   5. (opt)      → --install: adb install, grant permissions, launch
#
# Usage:
#   ./buildApk.sh                 # gates, then a release build
#   ./buildApk.sh --debug         # debug build
#   ./buildApk.sh --no-gates      # skip scripts/gates.sh
#   ./buildApk.sh --install       # build, then install on the Supernote
#   ./buildApk.sh --install --device 10.0.0.22:5555   # a specific adb serial
#
# adb over Wi-Fi (F1-FR6): the Supernote has one USB-C port, so a USB mic and an adb cable
# can't both be plugged in. Switch adb to Wi-Fi once per boot:
#   adb tcpip 5555                   # with the cable in
#   adb connect <device-ip>:5555     # then unplug; <device-ip> is its Wi-Fi address
#   ./buildApk.sh --install --device <device-ip>:5555
# =========================================================

PKG_ID="dev.jraghavan.snmeeting"
ABI="arm64-v8a"          # the Supernote is arm64; nothing else ships (F1-FR1)
ANDROID_PLATFORM="30"    # Android 11
LIB="libmeeting.so"
APK_BUDGET_BYTES=$((25 * 1024 * 1024))   # F1-FR10 recommendation; re-baseline after measuring

write_color_output() {
    local message="${1:-}" color="${2:-}"
    case "$color" in
        Red)    printf "\033[31m%s\033[0m\n" "$message" >&2 ;;
        Green)  printf "\033[32m%s\033[0m\n" "$message" >&2 ;;
        Yellow) printf "\033[33m%s\033[0m\n" "$message" >&2 ;;
        Blue)   printf "\033[34m%s\033[0m\n" "$message" >&2 ;;
        *)      printf "%s\n" "$message" >&2 ;;
    esac
}

die() { write_color_output "$1" "Red"; exit 1; }

# adb on the chosen device. ${arr[@]+"${arr[@]}"} expands to nothing for an empty array; bash
# 3.2 (macOS) errors on a bare "${arr[@]}" under `set -u`.
ADB_DEVICE=()
adb_dev() { adb ${ADB_DEVICE[@]+"${ADB_DEVICE[@]}"} "$@"; }

# =========================================================
# select_jdk — Gradle needs JDK 17–21; scripts/jdk.sh picks one (shared with scripts/gates.sh).
# =========================================================
select_jdk() {
    local root="$1" home
    # shellcheck source=scripts/jdk.sh
    source "$root/scripts/jdk.sh"
    home="$(gradle_jdk_home)" || die "Gradle needs JDK 17–21. Install JDK 21 or set JAVA_HOME."
    if [[ "$home" != "${JAVA_HOME:-}" ]]; then
        write_color_output "Using JDK $(jdk_major "$home/bin/java") for Gradle: $home" "Yellow"
    fi
    export JAVA_HOME="$home"
}

# =========================================================
# use_pinned_rust — a Homebrew cargo earlier on PATH ignores rust-toolchain.toml, and cargo-ndk
# would then build with rustc 1.85. `rustup which cargo`, run in the repo, honours the pin;
# put its bin dir first so cargo-ndk's child cargo/rustc are the pinned ones.
# =========================================================
use_pinned_rust() {
    local root="$1" cargo_path
    command -v rustup >/dev/null 2>&1 || die "rustup not found (https://rustup.rs)"
    cargo_path="$(cd "$root" && rustup which cargo)" || die "rustup has no toolchain for rust-toolchain.toml"
    export PATH="$(dirname "$cargo_path"):$PATH"
    write_color_output "rust: $(cargo --version) (pinned via rust-toolchain.toml)" "Blue"
}

# =========================================================
# use_ndk — default ANDROID_NDK_HOME to the newest NDK in the SDK.
# =========================================================
use_ndk() {
    local sdk="${ANDROID_HOME:-$HOME/Library/Android/sdk}"
    if [[ -z "${ANDROID_NDK_HOME:-}" ]]; then
        ANDROID_NDK_HOME="$(ls -d "$sdk"/ndk/*/ 2>/dev/null | sort -V | tail -1)"
        ANDROID_NDK_HOME="${ANDROID_NDK_HOME%/}"
    fi
    [[ -n "$ANDROID_NDK_HOME" && -d "$ANDROID_NDK_HOME" ]] || die "No NDK under $sdk/ndk; install one or set ANDROID_NDK_HOME"
    export ANDROID_NDK_HOME
    write_color_output "ndk: $ANDROID_NDK_HOME" "Blue"
}

run_gates() {
    local root="$1"
    write_color_output "Running quality gates (scripts/gates.sh)" "Blue"
    "$root/scripts/gates.sh" || die "Gates failed; fix them or pass --no-gates for a scratch build"
}

# =========================================================
# build_rust — the core into jniLibs with the JNI bridge on (host builds leave it off, INV5).
# =========================================================
build_rust() {
    local root="$1" jnilibs="$1/app/src/main/jniLibs"
    use_pinned_rust "$root"
    use_ndk
    cargo ndk --version >/dev/null 2>&1 || die "cargo-ndk not found: cargo install cargo-ndk"
    write_color_output "cargo ndk -t $ABI -P $ANDROID_PLATFORM build --release --features jni-bridge → $jnilibs" "Blue"
    ( cd "$root" && cargo ndk -t "$ABI" -P "$ANDROID_PLATFORM" -o "$jnilibs" \
        build --release -p meeting-core --features jni-bridge ) || die "cargo-ndk build failed"
    prune_stray_libs "$jnilibs"
    strip_lib "$jnilibs"
}

# cargo-ndk's -o copies every .so it builds; the APK needs only ours.
prune_stray_libs() {
    local dir="$1/$ABI" f
    while IFS= read -r f; do
        [[ -n "$f" ]] || continue
        rm -f "$f"
        write_color_output "pruned stray lib: $(basename "$f")" "Yellow"
    done < <(find "$1" -name '*.so' ! -path "$dir/$LIB" -print)
    [[ -f "$dir/$LIB" ]] || die "cargo-ndk produced no $dir/$LIB"
}

# Release is already stripped by Cargo; this makes sure, with the NDK's own tool.
strip_lib() {
    local lib="$1/$ABI/$LIB" strip_bin
    strip_bin="$(ls "$ANDROID_NDK_HOME"/toolchains/llvm/prebuilt/*/bin/llvm-strip 2>/dev/null | head -1)"
    [[ -n "$strip_bin" ]] || { write_color_output "llvm-strip not found in the NDK; left $LIB as built" "Yellow"; return 0; }
    "$strip_bin" --strip-unneeded "$lib" && write_color_output "stripped $LIB" "Green"
}

# =========================================================
# report_sizes — F1-FR10: record the .so and APK sizes, and prove the .so is packaged.
# =========================================================
report_sizes() {
    local lib="$1" apk="$2" lib_bytes apk_bytes entry
    lib_bytes="$(wc -c < "$lib" | tr -d ' ')"
    apk_bytes="$(wc -c < "$apk" | tr -d ' ')"
    entry="$(unzip -l "$apk" | grep "lib/$ABI/$LIB" || true)"
    [[ -n "$entry" ]] || die "lib/$ABI/$LIB is missing from $apk"
    write_color_output "size: $LIB $lib_bytes bytes ($(mb "$lib_bytes") MB)" "Blue"
    write_color_output "size: APK $apk_bytes bytes ($(mb "$apk_bytes") MB; budget $(mb "$APK_BUDGET_BYTES") MB)" "Blue"
    write_color_output "in APK: $(echo "$entry" | sed -E 's/^ +//; s/ +/ /g')" "Blue"
    if [[ "$apk_bytes" -gt "$APK_BUDGET_BYTES" ]]; then
        write_color_output "APK is over the F1-FR10 budget" "Yellow"
    fi
}

mb() { awk -v b="$1" 'BEGIN { printf "%.2f", b / 1048576 }'; }

# =========================================================
# audio_recording_state — F1-FR7 until RecordingService exists (M6): the spike records from
# MainActivity, so look for our package among AudioService's recordings. Reads `dumpsys audio`
# on stdin and prints yes / no / unknown.
#
# AOSP's RecordingActivityMonitor dumps "RecordActivityMonitor dump time: …", then per client
# "riid <n>; active <bool>" and an indented "session:… -- pack:<package> …" line, then an event
# log of past recordings (which must not count). Older releases list only the active clients,
# without riid lines. The format on the Supernote is DEVICE-UNVERIFIED, so anything we can't
# find is "unknown", which asks rather than passes.
# =========================================================
audio_recording_state() {
    awk -v pkg="pack:$PKG_ID" '
        /RecordActivityMonitor/ { insec = 1; seen = 1; state = "?"; next }
        insec && (tolower($0) ~ /event log/ || /^[A-Z]/) { insec = 0 }
        !insec { next }
        /^[ \t]*riid / { state = ($0 ~ /active true/) ? "on" : "off"; next }
        {
            i = index($0, pkg)
            if (i && substr($0, i + length(pkg), 1) !~ /[A-Za-z0-9._]/ && state != "off") active = 1
        }
        END { print (!seen ? "unknown" : active ? "yes" : "no") }'
}

# recording_state — yes / no / unknown for a recording by $PKG_ID on the device.
recording_state() {
    local services audio
    # Captured first: under pipefail, `adb | grep -q` can fail on SIGPIPE after a match.
    services="$(adb_dev shell dumpsys activity services "$PKG_ID" 2>/dev/null)" || { echo unknown; return 0; }
    if grep -q RecordingService <<< "$services"; then echo yes; return 0; fi
    audio="$(adb_dev shell dumpsys audio 2>/dev/null)" || { echo unknown; return 0; }
    audio_recording_state <<< "$audio"
}

# =========================================================
# confirm_no_recording — F1-FR7: installing force-stops the app, which ends a live recording
# (seen on the device: installPackageLI killed one). If a recording is running, or we couldn't
# tell, ask first; anything but "y" (including no terminal) cancels.
# =========================================================
confirm_no_recording() {
    local answer=""
    case "$(recording_state)" in
        no) return 0 ;;
        yes) write_color_output "A recording is in progress on the device; installing will stop it." "Yellow" ;;
        *) write_color_output "Couldn't check the device for a recording in progress." "Yellow" ;;
    esac
    read -r -p "Install anyway? [y/N] " answer || true
    case "$answer" in
        y|Y|yes|YES) write_color_output "Installing anyway" "Yellow" ;;
        *) die "Install cancelled" ;;
    esac
}

main() {
    local root; root="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
    local variant="release" install=0 gates=1

    while [[ $# -gt 0 ]]; do
        case "$1" in
            --debug)    variant="debug" ;;
            --install)  install=1 ;;
            --no-gates) gates=0 ;;
            --device)   [[ $# -ge 2 ]] || die "--device needs an adb serial"; ADB_DEVICE=(-s "$2"); shift ;;
            -h|--help)  sed -n '5,/^# ====/p' "${BASH_SOURCE[0]}" | sed '$d; s/^# \{0,1\}//'; exit 0 ;;
            *) die "Unknown arg: $1 (see --help)" ;;
        esac
        shift
    done

    select_jdk "$root"
    if [[ "$gates" == "1" ]]; then run_gates "$root"; fi
    build_rust "$root"

    # Capitalise portably (macOS ships bash 3.2, which lacks ${var^}).
    local task
    case "$variant" in
        release) task="assembleRelease" ;;
        debug)   task="assembleDebug" ;;
    esac
    ( cd "$root" && ./gradlew -q "$task" ) || die "gradle $task failed"
    local apk; apk="$(find "$root/app/build/outputs/apk/$variant" -name '*.apk' -print -quit)"
    [[ -n "$apk" ]] || die "No APK under app/build/outputs/apk/$variant"
    write_color_output "APK: $apk" "Green"
    report_sizes "$root/app/src/main/jniLibs/$ABI/$LIB" "$apk"

    if [[ "$install" == "1" ]]; then
        command -v adb >/dev/null 2>&1 || die "adb not found"
        confirm_no_recording
        adb_dev install -r "$apk" || die "adb install failed (see --help for adb over Wi-Fi)"
        adb_dev shell pm grant "$PKG_ID" android.permission.RECORD_AUDIO
        adb_dev shell appops set "$PKG_ID" MANAGE_EXTERNAL_STORAGE allow
        adb_dev shell mkdir -p /storage/emulated/0/EXPORT/sn-meeting
        adb_dev shell am start -n "$PKG_ID/.MainActivity"
        write_color_output "Installed and launched $PKG_ID" "Green"
    fi
}

# Sourced by scripts/test_install_guard.sh for its functions; run as a script, it builds.
if [[ "${BASH_SOURCE[0]}" == "$0" ]]; then
    main "$@"
fi
