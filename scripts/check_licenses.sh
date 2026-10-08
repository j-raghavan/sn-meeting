#!/bin/bash
set -euo pipefail
# F9-FR7 / S9: every crate linked into libmeeting.so (the Android target with jni-bridge,
# normal dependencies only) is listed in LICENSES-3RDPARTY.md. Run by scripts/gates.sh; prints
# each missing crate. Regenerate the table with:
#   cargo metadata --format-version 1 --filter-platform aarch64-linux-android --features jni-bridge \
#     --manifest-path meeting-core/Cargo.toml --locked --offline | python3 -I scripts/linked_crates.py table

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cargo metadata --format-version 1 --filter-platform aarch64-linux-android --features jni-bridge \
    --manifest-path "$ROOT/meeting-core/Cargo.toml" --locked --offline \
    | python3 -I "$ROOT/scripts/linked_crates.py" check "$ROOT/LICENSES-3RDPARTY.md"
