# Sourced by buildApk.sh and scripts/gates.sh: pick a JDK Gradle can run (17–21).

# jdk_major <java binary> — the major version, or nothing.
jdk_major() {
    "$1" -version 2>&1 | sed -nE 's/.*version "([0-9]+).*/\1/p' | head -1 || true
}

# gradle_jdk_home — print a JDK home for Gradle: the current one (JAVA_HOME, else java on PATH)
# if it is 17–21, else an installed 21, then 17. `java_home -F` fails when that version is
# missing; without -F it silently returns the newest JDK instead. Returns 1 if none is found.
gradle_jdk_home() {
    local java_bin="java" major v home
    [[ -n "${JAVA_HOME:-}" ]] && java_bin="$JAVA_HOME/bin/java"
    major="$(jdk_major "$java_bin")"
    if [[ -n "$major" && "$major" -ge 17 && "$major" -le 21 ]]; then
        if [[ -n "${JAVA_HOME:-}" ]]; then echo "$JAVA_HOME"; return 0; fi
        home="$(/usr/libexec/java_home -F -v "$major" 2>/dev/null)" && { echo "$home"; return 0; }
    fi
    for v in 21 17; do
        home="$(/usr/libexec/java_home -F -v "$v" 2>/dev/null)" || continue
        major="$(jdk_major "$home/bin/java")"
        [[ "$major" == "$v" ]] && { echo "$home"; return 0; }
    done
    return 1
}
