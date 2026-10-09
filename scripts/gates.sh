#!/bin/bash
set -euo pipefail
# Quality gates (ADR D8, NFR5): run before every commit and by buildApk.sh before a build.
# Usage: scripts/gates.sh

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

step() { printf '\033[34m== %s\033[0m\n' "$*" >&2; }
die() { printf '\033[31m%s\033[0m\n' "$*" >&2; exit 1; }

# A Homebrew cargo earlier on PATH ignores rust-toolchain.toml (and `rustup run 1.95.0 cargo`
# still picks it up), so put the pinned toolchain's bin dir first. Run from the repo root so
# `rustup which` honours rust-toolchain.toml. If it fails, stop: `dirname ""` would put "." on PATH.
CARGO_BIN="$(rustup which cargo)" || die "rustup has no toolchain for rust-toolchain.toml"
[[ -x "$CARGO_BIN" ]] || die "rustup which cargo gave '$CARGO_BIN'"
export PATH="$(dirname "$CARGO_BIN"):$PATH"

# shellcheck source=scripts/jdk.sh
source "$ROOT/scripts/jdk.sh"
JDK="$(gradle_jdk_home)" || die "Gradle needs JDK 17–21. Install JDK 21 or set JAVA_HOME."

step "cargo fmt --check";            cargo fmt --check
step "cargo clippy";                 cargo clippy --all-targets -- -D warnings
step "cargo clippy (jni-bridge)";    cargo clippy --all-targets --features jni-bridge -- -D warnings
# --all-targets unifies dev-dependency features into the library; build it alone too, as
# buildApk.sh does, so a feature only dev-dependencies enable can't hide.
step "cargo build (library alone)";  cargo build -p meeting-core --lib --features jni-bridge
step "cargo test";                   cargo test -p meeting-core
step "cargo llvm-cov (>= 97% lines)"
cargo llvm-cov -p meeting-core --fail-under-lines 97 --ignore-filename-regex 'meeting-core/src/jni\.rs$'
step "install guard (stub adb)";     "$ROOT/scripts/test_install_guard.sh"
step "check-audio (tone, silence, noise, speech, clipped)"; "$ROOT/scripts/test_check_audio.sh"
step "licences of linked crates";    "$ROOT/scripts/check_licenses.sh"
step "gradle lintRelease (JDK $JDK)"
JAVA_HOME="$JDK" ./gradlew -q lintRelease
step "gradle :app:testDebugUnitTest"
JAVA_HOME="$JDK" ./gradlew -q :app:testDebugUnitTest
printf '\033[32mAll gates passed\033[0m\n' >&2
