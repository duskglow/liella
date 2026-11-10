#!/usr/bin/env python3
"""Drop the hot-text keys that no build matches (STARTUP-FAULTS-RESTORE).

usage: prune.py <hot-text-<profile>.list> <hot-text.matched>... > new list

Each <hot-text.matched> is the key file `cargo xtask so` writes next to a profile's processed
archive (target/product/<profile>/hot-text.matched). A key of the list that is in none of them
places nothing (renamed, made generic, inlined everywhere or removed): it is dropped. Comments and
every other line are kept as they are, in order. Each profile has its own list (HOTTEXT-REGEN-HOSTS-2,
D-X.61): prune crates/libc-so/hot-text-<profile>.list with that profile's matched file. Prints the
dropped keys on stderr.
"""
import sys


def main():
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
