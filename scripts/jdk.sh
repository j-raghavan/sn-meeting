# Sourced by buildApk.sh and scripts/gates.sh: pick a JDK Gradle can run (17–21).

# jdk_major <java binary> — the major version, or nothing.
jdk_major() {
    "$1" -version 2>&1 | sed -nE 's/.*version "([0-9]+).*/\1/p' | head -1 || true
}

# path_jdk_home — the JDK home of the java on PATH, by resolving its symlinks (Linux has no
# /usr/libexec/java_home). Prints nothing if java isn't on PATH.
path_jdk_home() {
    local java_bin
    java_bin="$(command -v java 2>/dev/null)" || return 0
    java_bin="$(readlink -f "$java_bin" 2>/dev/null)" || return 0
    dirname "$(dirname "$java_bin")"
}

# gradle_jdk_home — print a JDK home for Gradle: the current one (JAVA_HOME, else java on PATH)
# if it is 17–21, else an installed 21, then 17. `java_home -F` fails when that version is
# missing; without -F it silently returns the newest JDK instead. Returns 1 if none is found.
# Without /usr/libexec/java_home (Linux, CI), JAVA_HOME or the java on PATH must be 17–21.
gradle_jdk_home() {
    local java_bin="java" major v home
    [[ -n "${JAVA_HOME:-}" ]] && java_bin="$JAVA_HOME/bin/java"
    major="$(jdk_major "$java_bin")"
    if [[ -n "$major" && "$major" -ge 17 && "$major" -le 21 ]]; then
        if [[ -n "${JAVA_HOME:-}" ]]; then echo "$JAVA_HOME"; return 0; fi
        if [[ ! -x /usr/libexec/java_home ]]; then
            home="$(path_jdk_home)"
            [[ -n "$home" && -x "$home/bin/java" ]] && { echo "$home"; return 0; }
            return 1
        fi
        home="$(/usr/libexec/java_home -F -v "$major" 2>/dev/null)" && { echo "$home"; return 0; }
    fi
    [[ -x /usr/libexec/java_home ]] || return 1
    for v in 21 17; do
        home="$(/usr/libexec/java_home -F -v "$v" 2>/dev/null)" || continue
        major="$(jdk_major "$home/bin/java")"
        [[ "$major" == "$v" ]] && { echo "$home"; return 0; }
    done
    return 1
}
