#!/usr/bin/env python3
"""Drop the hot-text keys that no build matches (STARTUP-FAULTS-RESTORE).

usage: prune.py <hot-text.list> <hot-text.matched>... > new list

Each <hot-text.matched> is the key file `cargo xtask so` writes next to a profile's processed
archive (target/product/<profile>/hot-text.matched). A key of the list that is in none of them
places nothing (renamed, made generic, inlined everywhere or removed): it is dropped. Comments and
every other line are kept as they are, in order. Pass the files of BOTH profiles (fast, release):
a key one profile inlines is still live in the other. Prints the dropped keys on stderr.

usage: prune.py --absent <hot-text.list> <hot-text.absent> > new allow-list

LAYOUT-TRACE-HOST-CANONICAL-3: the lines of <hot-text.absent> whose key is no longer a key of
<hot-text.list> are stale by its own rule (`cargo xtask so` refuses them: "not in hot-text.list"),
so they are dropped; comments and every other line are kept as they are, in order. regen.sh
--replace runs it after writing the list (a replaced list drops the keys no trace hits, and their
allow-list lines with them). Prints the dropped lines on stderr.
"""
import sys


def absent(list_path, absent_path):
    with open(list_path) as f:
        listed = {l.split("\t", 1)[0].strip() for l in f if not l.startswith("#") and l.strip()}
    dropped = 0
    with open(absent_path) as f:
        for line in f:
            key = line.split("\t", 1)[0].strip()
            if line.startswith("#") or not key or key in listed:
                sys.stdout.write(line)
            else:
                dropped += 1
                print("absent: dropped", line.rstrip("\n"), file=sys.stderr)
    print(f"prune: {dropped} allow-list line(s) dropped (key no longer listed)", file=sys.stderr)


def main():
    if len(sys.argv) == 4 and sys.argv[1] == "--absent":
        absent(sys.argv[2], sys.argv[3])
        return
    if len(sys.argv) < 3:
        sys.exit(__doc__)
    live = set()
    for path in sys.argv[2:]:
        with open(path) as f:
            live.update(l.strip() for l in f if l.strip() and not l.startswith("#"))
    dropped = 0
    with open(sys.argv[1]) as f:
        for line in f:
            key = line.split("\t", 1)[0].strip()
            if line.startswith("#") or not key or key in live:
                sys.stdout.write(line)
            else:
                dropped += 1
                print("dropped", key, file=sys.stderr)
    print(f"prune: {dropped} keys dropped", file=sys.stderr)


if __name__ == "__main__":
    main()
