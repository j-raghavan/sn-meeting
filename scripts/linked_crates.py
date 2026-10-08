"""S9 / F9-FR7: the crates linked into libmeeting.so, from `cargo metadata` on stdin (run with
--filter-platform aarch64-linux-android --features jni-bridge). Normal dependencies only,
walked from meeting-core; build scripts' and proc-macros' own dependencies run on the host and
are not linked, so they are left out (a proc-macro crate itself is not linked either).

  python3 -I scripts/linked_crates.py table       markdown rows: | crate | version | license |
  python3 -I scripts/linked_crates.py check FILE  exit 1 naming each crate FILE doesn't mention
"""
import json
import re
import sys


def linked(meta):
    pkgs = {p["id"]: p for p in meta["packages"]}
    nodes = {n["id"]: n for n in meta["resolve"]["nodes"]}
    root = next(p["id"] for p in meta["packages"] if p["name"] == "meeting-core")
    seen, todo = set(), [root]
    while todo:
        nid = todo.pop()
        for d in nodes[nid]["deps"]:
            normal = any(k["kind"] is None for k in d["dep_kinds"])
            if not normal or d["pkg"] in seen:
                continue
            kinds = {k for t in pkgs[d["pkg"]]["targets"] for k in t["kind"]}
            if "proc-macro" in kinds:
                continue
            seen.add(d["pkg"])
            todo.append(d["pkg"])
    return sorted((pkgs[i] for i in seen), key=lambda p: (p["name"], p["version"]))


def main():
    meta = json.load(sys.stdin)
    crates = linked(meta)
    if sys.argv[1:2] == ["table"]:
        for p in crates:
            print(f"| {p['name']} | {p['version']} | {p.get('license') or 'see its repository'} |")
        return 0
    if sys.argv[1:2] == ["check"] and len(sys.argv) == 3:
        with open(sys.argv[2], encoding="utf-8") as f:
            text = f.read()
        named = set(re.findall(r"^\| ([A-Za-z0-9_-]+) \|", text, re.M))
        missing = sorted({p["name"] for p in crates} - named)
        for m in missing:
            print(f"{sys.argv[2]} doesn't list {m}", file=sys.stderr)
        return 1 if missing else 0
    print(__doc__, file=sys.stderr)
    return 2


if __name__ == "__main__":
    sys.exit(main())
