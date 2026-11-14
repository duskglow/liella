#!/usr/bin/env python3
"""Write one trace of one build as an ordered hot-text list (HOT-ORDER, HOTTEXT-STARTUP-COMPLETE):
order.py <new keys> <profile> > list.

<new keys>: `<key>\t<tier>` lines in first-touch order, from one trace of <profile> (fast or release).
Output: `<key>\t<tier>\t<profile>` lines, in first-touch order; a key traced twice keeps its first tier.

LAYOUT-DELETE-LISTS (LT-5): every product build traces itself (`cargo xtask so`, D-X.88 / D-X.92) and
writes its lists under target/product/<profile>; nothing is merged with an earlier list (the committed
lists and their merge mode are gone), so no function name crosses a build.

`--startup-head <startup-text.keys>` (regen.sh, ruling #4417, D-X.63): every key of that file the
list holds takes tier `startup`, and the start-up tier moves to the head of the list (both parts keep
their order). The start-up text is then contiguous: an aligned hot-text cluster can never sit between
two start-up keys and push the rest into another 64 KiB window (STARTUP-TEXT-WINDOWS, D-X.49). Keys of
that file the list lacks are not added (list order only).
`--alias <file>` (`keys.py --aliases` lines, `<symbol key>\t<section key>`; LAYOUT-TEXT-PIN-6, #4467):
a start-up key the list lacks that is a label inside a listed section (an assembly entry label, an
alias) brings that section's key to the head instead.
"""
import sys

TIERS = ["startup", "fork", "stdio", "strings", "locale", "tool", "extra"]
PROFILES = ["fast", "release"]


def read_keys(path, columns):
    """`<key>\t<tier>` lines (columns 2) or `<key>\t<tier>\t<profiles>` lines (columns 3); `#` lines skipped."""
    out = {}
    with open(path) as f:
        for line in f:
            line = line.rstrip("\n")
            if not line.strip() or line.startswith("#"):
                continue
            parts = line.split("\t")
            if len(parts) != columns or parts[1] not in TIERS:
                sys.exit(f"order.py: {path}: bad line {line!r} (want {columns} tab-separated fields, a tier of {TIERS} second)")
            out.setdefault(parts[0], parts[1])
    return out


def take(args, flag):
    if flag not in args:
        return None
    i = args.index(flag)
    if i + 1 >= len(args):
        sys.exit(f"order.py: {flag} needs a file")
    value = args[i + 1]
    del args[i:i + 2]
    return value


def main():
    args = sys.argv[1:]
    alias = {}
    alias_path = take(args, "--alias")
    if alias_path is not None:
        with open(alias_path) as f:
            for line in f:
                parts = line.rstrip("\n").split("\t")
                if len(parts) != 2:
                    sys.exit(f"order.py: {alias_path}: bad line {line!r} (want <symbol key>\\t<section key>)")
                alias[parts[0]] = parts[1]
    head_path = take(args, "--startup-head")
    head = set(read_keys(head_path, 3)) if head_path is not None else None
    if len(args) != 2:
        sys.exit("usage: order.py <new keys> <profile> [--startup-head <startup-text.keys>] [--alias <file>]")
    new_path, profile = args
    if profile not in PROFILES:
        sys.exit(f"order.py: unknown profile {profile!r} (want one of {PROFILES})")
    tier = read_keys(new_path, 2)
    out = list(tier)
    if head is not None:
        head |= {alias[k] for k in head if k not in tier and k in alias}
        for k in out:
            if k in head:
                tier[k] = "startup"
        out = [k for k in out if tier[k] == "startup"] + [k for k in out if tier[k] != "startup"]
    for k in out:
        print(f"{k}\t{tier[k]}\t{profile}")


if __name__ == "__main__":
    main()
