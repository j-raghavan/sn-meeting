#!/bin/bash
set -euo pipefail
# Quality gates (ADR D8, NFR5): run before every commit and by buildApk.sh before a build.
# Usage: scripts/gates.sh

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

# A Homebrew cargo earlier on PATH ignores rust-toolchain.toml (and `rustup run 1.95.0 cargo`
# still picks it up), so put the pinned toolchain's bin dir first. Run from the repo root so
# `rustup which` honours rust-toolchain.toml.
export PATH="$(dirname "$(rustup which cargo)"):$PATH"

step() { printf '\033[34m== %s\033[0m\n' "$*" >&2; }

step "cargo fmt --check";            cargo fmt --check
step "cargo clippy";                 cargo clippy --all-targets -- -D warnings
step "cargo test";                   cargo test -p meeting-core
step "cargo llvm-cov (>= 97% lines)"
cargo llvm-cov -p meeting-core --fail-under-lines 97 --ignore-filename-regex 'meeting-core/src/jni\.rs$'
step "gradle lintRelease"
JAVA_HOME="$(/usr/libexec/java_home -v 21)" ./gradlew -q lintRelease
printf '\033[32mAll gates passed\033[0m\n' >&2
