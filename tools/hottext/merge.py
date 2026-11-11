#!/usr/bin/env python3
"""Merge an ordered hot-text list with a new trace (HOT-ORDER, HOTTEXT-STARTUP-COMPLETE):
merge.py <old list lines> <new keys> <profile> > merged.

<new keys>: `<key>\t<tier>` lines in first-touch order, from one trace of <profile> (fast or release).
<old list lines>: `<key>\t<tier>\t<profiles>` lines (profiles comma-separated); a bare `<key>` line
(the list before tiers) has no tier and no profile.
Output: `<key>\t<tier>\t<profiles>` lines.

The new trace's first-touch order wins. An old key the new trace did not hit (the other build
profile's inlining, a probe that changed) goes right after the nearest key that preceded it in the
old list, or first if none did, so a union over both profiles keeps each profile's touch order.
The new trace is the whole truth for <profile>: an old key it did not hit loses <profile>, and a key
left with no profile (traced only in <profile>, now renamed, generic or inlined there) is dropped.
A bare old key keeps its place with tier `extra` and no profile (`-`); a key with no profile is
kept (unguarded) until a trace hits it. `regen.sh --replace` starts from an empty list.
A key's tier is the earliest (TIERS order) of the tiers it was traced with in its profiles.

`--startup-head <startup-text.keys>` (regen.sh, ruling #4417, D-X.63): every key of that file the
merged list holds takes tier `startup`, and the start-up tier moves to the head of the list (both
parts keep their merged order). The start-up text is then contiguous: an aligned hot-text cluster
can never sit between two start-up keys and push the rest into another 64 KiB window
(STARTUP-TEXT-WINDOWS, D-X.49). Keys of that file the list lacks are not added (list order only).
`--alias <file>` (`keys.py --aliases` lines, `<symbol key>\t<section key>`; LAYOUT-TEXT-PIN-6, #4467):
a start-up key the list lacks that is a label inside a listed section (an assembly entry label, an
alias) brings that section's key to the head instead.
`--dropped <file>` (LAYOUT-TEXT-PIN-9, ruling #4539): writes the old keys the merge dropped
(`<key>\t<tier>\t<old profiles>`, empty file if none); each is also printed to stderr. regen.sh
refuses a list that drops a key unless given --allow-drop.
"""
import sys

TIERS = ["startup", "fork", "stdio", "strings", "locale", "tool", "extra"]
PROFILES = ["fast", "release"]


def read_old(path):
    out = []
    with open(path) as f:
        for line in f:
            line = line.rstrip("\n")
            if not line.strip() or line.startswith("#"):
                continue
            parts = line.split("\t")
            if len(parts) == 1:
                out.append((parts[0].strip(), None, set()))
            elif len(parts) == 3 and parts[1] in TIERS:
                profs = set() if parts[2] == "-" else set(parts[2].split(","))
                if not profs <= set(PROFILES):
                    sys.exit(f"merge.py: {path}: unknown profile in {line!r}")
                out.append((parts[0], parts[1], profs))
            else:
                sys.exit(f"merge.py: {path}: bad line {line!r} (want <key>\\t<tier>\\t<profiles>)")
    return out


def read_new(path):
    out = {}
    with open(path) as f:
        for line in f:
            line = line.rstrip("\n")
            if not line.strip():
                continue
            k, tier = line.split("\t")
            if tier not in TIERS:
                sys.exit(f"merge.py: {path}: unknown tier in {line!r}")
            out.setdefault(k, tier)
    return out


def main():
    args = sys.argv[1:]
    head, alias, dropped_path = None, {}, None
    if "--dropped" in args:
        i = args.index("--dropped")
        if i + 1 >= len(args):
            sys.exit("merge.py: --dropped needs a file")
        dropped_path = args[i + 1]
        del args[i:i + 2]
    if "--alias" in args:
        i = args.index("--alias")
        if i + 1 >= len(args):
            sys.exit("merge.py: --alias needs a file")
        with open(args[i + 1]) as f:
            for line in f:
                parts = line.rstrip("\n").split("\t")
                if len(parts) != 2:
                    sys.exit(f"merge.py: {args[i + 1]}: bad line {line!r} (want <symbol key>\\t<section key>)")
                alias[parts[0]] = parts[1]
        del args[i:i + 2]
    if "--startup-head" in args:
        i = args.index("--startup-head")
        if i + 1 >= len(args):
            sys.exit("merge.py: --startup-head needs a file")
        head = {k for k, _, _ in read_old(args[i + 1])}
        del args[i:i + 2]
    if len(args) != 3:
        sys.exit("usage: merge.py <old list lines> <new keys> <profile> [--startup-head <startup-text.keys>] [--alias <file>] [--dropped <file>]")
    old_path, new_path, profile = args
    if profile not in PROFILES:
        sys.exit(f"merge.py: unknown profile {profile!r} (want one of {PROFILES})")
    old, new = read_old(old_path), read_new(new_path)
    tier = dict(new)
    profs = {k: {profile} for k in new}
    out = list(new)
    pos = {k: i for i, k in enumerate(out)}
    anchor = None  # the last old key already placed
    dropped = []
    for k, t, ps in old:
        legacy = t is None
        others = ps - {profile}
        if k in pos:
            # Traced now; keep the other profiles and the earlier of the two tiers they agree on.
            if others and not legacy:
                profs[k] |= others
                tier[k] = min(tier[k], t, key=TIERS.index)
            anchor = k
            continue
        if ps and not others:
            dropped.append(f"{k}\t{t}\t{','.join(x for x in PROFILES if x in ps)}")
            continue  # traced only in this profile, and no longer: stale
        i = pos[anchor] + 1 if anchor is not None else 0
        out.insert(i, k)
        pos = {k2: j for j, k2 in enumerate(out)}
        tier[k] = "extra" if legacy else t
        profs[k] = others
        anchor = k
    if head is not None:
        head |= {alias[k] for k in head if k not in pos and k in alias}
        for k in out:
            if k in head:
                tier[k] = "startup"
        out = [k for k in out if tier[k] == "startup"] + [k for k in out if tier[k] != "startup"]
    for k in out:
        p = ",".join(x for x in PROFILES if x in profs[k]) or "-"
        print(f"{k}\t{tier[k]}\t{p}")
    # LAYOUT-TEXT-PIN-9 (ruling #4539): a dropped key is never silent. Every one goes to stderr,
    # and to the --dropped file, where regen.sh refuses the new list unless told --allow-drop.
    for d in dropped:
        print(f"merge.py: DROPPED (no {profile} trace hit it, no other profile keeps it): {d}", file=sys.stderr)
    if dropped_path is not None:
        with open(dropped_path, "w") as f:
            f.writelines(d + "\n" for d in dropped)


if __name__ == "__main__":
    main()
