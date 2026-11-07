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
    old_path, new_path, profile = sys.argv[1:4]
    if profile not in PROFILES:
        sys.exit(f"merge.py: unknown profile {profile!r} (want one of {PROFILES})")
    old, new = read_old(old_path), read_new(new_path)
    tier = dict(new)
    profs = {k: {profile} for k in new}
    out = list(new)
    pos = {k: i for i, k in enumerate(out)}
    anchor = None  # the last old key already placed
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
            continue  # traced only in this profile, and no longer: stale
        i = pos[anchor] + 1 if anchor is not None else 0
        out.insert(i, k)
        pos = {k2: j for j, k2 in enumerate(out)}
        tier[k] = "extra" if legacy else t
        profs[k] = others
        anchor = k
    for k in out:
        p = ",".join(x for x in PROFILES if x in profs[k]) or "-"
        print(f"{k}\t{tier[k]}\t{p}")


if __name__ == "__main__":
    main()
