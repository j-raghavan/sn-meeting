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
#   4. (opt)      → --install: adb install, grant permissions, launch
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
# select_jdk — Gradle needs JDK 17–21. If the JDK it would use (JAVA_HOME, else java on PATH)
# is newer, switch to an installed 21 (then 17), as inkread does.
# =========================================================
select_jdk() {
    local java_bin="java" jmajor jh v
    [[ -n "${JAVA_HOME:-}" ]] && java_bin="$JAVA_HOME/bin/java"
    jmajor="$("$java_bin" -version 2>&1 | sed -nE 's/.*version "([0-9]+).*/\1/p' | head -1 || true)"
    [[ -n "$jmajor" && "$jmajor" -ge 17 && "$jmajor" -le 21 ]] && return 0
    for v in 21 17; do
        if jh="$(/usr/libexec/java_home -v "$v" 2>/dev/null)"; then
            export JAVA_HOME="$jh"
            write_color_output "JDK ${jmajor:-?} unsupported by Gradle; using JDK $v: $JAVA_HOME" "Yellow"
            return 0
        fi
    done
    die "Gradle needs JDK 17–21 (found ${jmajor:-none}). Install JDK 21 or set JAVA_HOME."
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

    select_jdk
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

    if [[ "$install" == "1" ]]; then
        command -v adb >/dev/null 2>&1 || die "adb not found"
        adb_dev install -r "$apk" || die "adb install failed (see --help for adb over Wi-Fi)"
        adb_dev shell pm grant "$PKG_ID" android.permission.RECORD_AUDIO
        adb_dev shell appops set "$PKG_ID" MANAGE_EXTERNAL_STORAGE allow
        adb_dev shell mkdir -p /storage/emulated/0/EXPORT/sn-meeting
        adb_dev shell am start -n "$PKG_ID/.MainActivity"
        write_color_output "Installed and launched $PKG_ID" "Green"
    fi
}

main "$@"
