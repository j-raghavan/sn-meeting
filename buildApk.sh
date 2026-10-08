#!/bin/bash
set -euo pipefail
# Build the Meetings APK; --install also installs it on the connected Supernote and grants permissions.
# Usage: ./buildApk.sh [--install]

PKG_ID="dev.jraghavan.snmeeting"
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

# Gradle needs JDK 17–21; pick 21 if the active java is newer (same guard as inkread).
if [[ -z "${JAVA_HOME:-}" ]]; then
    jmajor="$(java -version 2>&1 | sed -nE 's/.*version "([0-9]+).*/\1/p' | head -1)"
    if [[ -n "$jmajor" && "$jmajor" -gt 23 ]]; then
        export JAVA_HOME="$(/usr/libexec/java_home -v 21)"
    fi
fi

( cd "$ROOT" && ./gradlew assembleRelease )
APK="$(find "$ROOT/app/build/outputs/apk/release" -name '*.apk' -print -quit)"
echo "APK: $APK"

if [[ "${1:-}" == "--install" ]]; then
    adb install -r "$APK"
    adb shell pm grant "$PKG_ID" android.permission.RECORD_AUDIO
    adb shell appops set "$PKG_ID" MANAGE_EXTERNAL_STORAGE allow
    adb shell mkdir -p /storage/emulated/0/EXPORT/sn-meeting
    adb shell am start -n "$PKG_ID/.MainActivity"
fi
