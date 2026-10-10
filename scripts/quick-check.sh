#!/bin/bash
set -euo pipefail
# The fast check before a push (owner, 2026-10-10): most of what CI checks, in minutes, so CI
# confirms rather than discovers. scripts/gates.sh stays the full run (it repeats the whole test
# suite under coverage); CI runs that on every push.
#
#   scripts/quick-check.sh            check the working tree and the commits not yet pushed
#   scripts/quick-check.sh --hook     the same, from .githooks/pre-push
#
# What it runs:
#   - cargo fmt --check, clippy (with and without jni-bridge), and every test compiled
#   - the core's unit tests, and the contract and leak tests on every run
#   - the integration tests that use a module changed since the upstream branch, or that changed
#   - Kotlin: the JVM unit tests when anything under app/ changed
#   - the licence check when Cargo.lock changed; the script tests when scripts/ changed
#   - commit messages not yet pushed: a requirement id, at most 72 characters, no AI attribution
#     (the same rules as .github/workflows/commit-checks.yml)

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

step() { printf '\033[34m== %s\033[0m\n' "$*" >&2; }
die() { printf '\033[31m%s\033[0m\n' "$*" >&2; exit 1; }

CARGO_BIN="$(rustup which cargo)" || die "rustup has no toolchain for rust-toolchain.toml"
[[ -x "$CARGO_BIN" ]] || die "rustup which cargo gave '$CARGO_BIN'"
export PATH="$(dirname "$CARGO_BIN"):$PATH"

# What "not yet pushed" means: the upstream branch if there is one, else origin/main.
UPSTREAM="$(git rev-parse --abbrev-ref --symbolic-full-name '@{u}' 2>/dev/null || echo origin/main)"
BASE="$(git merge-base HEAD "$UPSTREAM" 2>/dev/null || git rev-parse "$UPSTREAM")"
# Changed files: committed since the base, plus anything staged or modified.
CHANGED="$( { git diff --name-only "$BASE" HEAD; git diff --name-only HEAD; } | sort -u)"

# ---- commit messages (commit-checks.yml) -------------------------------------------------------
step "commit messages since $UPSTREAM"
ID='(F[0-9]+-(FR|AC)[0-9]+|INV[0-9]+|EC[0-9]+|NFR[0-9]+|ADR D[0-9]+|Security|Deps|Release)'
PATTERN="^${ID}(, ${ID})*: [^ ].*\$"
bad=0
while read -r sha; do
    [[ -z "$sha" ]] && continue
    subject="$(git log -1 --format=%s "$sha")"
    grep -qE "$PATTERN" <<< "$subject" || { echo "  no requirement id: '$subject' (${sha:0:7})" >&2; bad=1; }
    (( ${#subject} > 72 )) && { echo "  over 72 characters (${#subject}): '$subject' (${sha:0:7})" >&2; bad=1; }
    if git log -1 --format=%B "$sha" | grep -qiE 'co-authored-by|generated with|claude|anthropic|🤖'; then
        echo "  AI attribution in ${sha:0:7}" >&2; bad=1
    fi
done < <(git rev-list --no-merges "$BASE..HEAD")
(( bad == 0 )) || die "fix the commit messages above (git commit --amend / rebase) before pushing"

# ---- Rust --------------------------------------------------------------------------------------
step "cargo fmt --check";                cargo fmt --check
step "cargo clippy";                     cargo clippy --all-targets -- -D warnings
step "cargo clippy (jni-bridge)";        cargo clippy --all-targets --features jni-bridge -- -D warnings
step "compile every test";               cargo test -p meeting-core --no-run -q
step "core unit tests";                  cargo test -p meeting-core --lib -q

# Contract and leak tests run every time: they guard the JNI surface, the host graph and secrets.
TESTS=(api_global jni_names inv5 m4_key_leak m13_leak scrub no_shared_client)

# Integration tests that changed, or that use a module that changed.
while read -r f; do
    case "$f" in
        meeting-core/tests/*.rs) TESTS+=("$(basename "$f" .rs)") ;;
        meeting-core/src/*.rs)
            m="${f#meeting-core/src/}"; m="${m%.rs}"; m="${m%/mod}"; m="${m%%/*}"
            [[ "$m" == "lib" || "$m" == "api" || "$m" == "jni" ]] && continue
            while read -r t; do TESTS+=("$(basename "$t" .rs)"); done < <(
                grep -lE "meeting_core::${m}([^a-z0-9_]|\$)|use meeting_core::\{[^}]*[^a-z0-9_]${m}([^a-z0-9_]|\$)" meeting-core/tests/*.rs 2>/dev/null || true)
            ;;
    esac
done <<< "$CHANGED"
# api.rs and jni.rs are the bridge: anything there runs the API tests too.
if grep -qE '^meeting-core/src/(api|jni)\.rs$|^meeting-core/src/api/' <<< "$CHANGED"; then
    while read -r t; do TESTS+=("$(basename "$t" .rs)"); done < <(ls meeting-core/tests/*_api.rs 2>/dev/null)
fi
# macOS /bin/bash is 3.2: no mapfile, so de-duplicate with a read loop.
ARGS=(); RAN=""
while read -r t; do
    [[ -f "meeting-core/tests/$t.rs" ]] || continue
    ARGS+=(--test "$t"); RAN="$RAN $t"
done < <(printf '%s\n' "${TESTS[@]}" | sort -u)
step "integration tests:$RAN"
cargo test -p meeting-core -q "${ARGS[@]}"

# ---- the rest, only when it changed ------------------------------------------------------------
if grep -q '^Cargo.lock$' <<< "$CHANGED"; then
    step "licences of linked crates";    "$ROOT/scripts/check_licenses.sh"
fi
if grep -q '^scripts/' <<< "$CHANGED"; then
    step "install guard";                "$ROOT/scripts/test_install_guard.sh"
    step "check-audio";                  "$ROOT/scripts/test_check_audio.sh"
fi
if grep -q '^app/' <<< "$CHANGED"; then
    # shellcheck source=scripts/jdk.sh
    source "$ROOT/scripts/jdk.sh"
    JDK="$(gradle_jdk_home)" || die "Gradle needs JDK 17–21. Install JDK 21 or set JAVA_HOME."
    step "gradle :app:testDebugUnitTest (JDK $JDK)"
    JAVA_HOME="$JDK" ./gradlew -q :app:testDebugUnitTest
fi

printf '\033[32mQuick check passed\033[0m (CI still runs the full suite, coverage and lint)\n'
