#!/usr/bin/env python3
"""Merge an ordered hot-text list with a new trace (HOT-ORDER, HOTTEXT-STARTUP-COMPLETE,
HOTTEXT-REGEN-HOSTS): merge.py <old list lines> <new keys> <profile> <host class> > merged.

<new keys>: `<key>\t<tier>` lines in first-touch order, from one trace of <profile> (fast or
release) on a host of <host class> (regen.sh: `<vendor>-<family>-<model>` from /proc/cpuinfo,
e.g. `intel-6-85` Skylake-SP, `amd-23-1` Zen 1).
<old list lines>: `<key>\t<tier>\t<profiles>\t<provenance>` lines. <provenance> is the
comma-separated `<profile>@<host class>` pairs whose trace hit the key; <profiles> is the set of
their profiles (`-` for none), the column xtask/src/hottext.rs checks. A line without the
provenance column (the list before HOTTEXT-REGEN-HOSTS) has `<profile>@?` for each of its
profiles: hit by an unrecorded host class. A bare `<key>` line has no tier and no profile.
Output: `<key>\t<tier>\t<profiles>\t<provenance>` lines.

The list is the union over every host class traced: dispatch (fill_erms needs ERMS, which Zen 1
lacks), locale and catalog paths differ by host, and one host's trace is not the whole truth.
A trace is the whole truth for its own (<profile>, <host class>) pair only: an old key it did not
hit loses that pair, and a key is dropped only when no pair is left (no host class of any
profile reaches it any more). A hit replaces `<profile>@?` by `<profile>@<host class>`; a miss
leaves `<profile>@?` in place (the unrecorded host may reach it), so only `regen.sh --replace`
or `prune.py` removes a key whose provenance is unrecorded.
The new trace's first-touch order wins. An old key the new trace did not hit (another profile's
inlining, another host class's dispatch, a probe that changed) goes right after the nearest key
that preceded it in the old list, or first if none did, so the union keeps each trace's touch
order. A bare old key keeps its place with tier `extra` and no profile (`-`); a key with no
profile is kept (unguarded) until a trace hits it.
A key's tier is the earliest (TIERS order) of the tiers it was traced with by its pairs.
"""
import re
import sys

TIERS = ["startup", "fork", "stdio", "strings", "locale", "tool", "extra"]
PROFILES = ["fast", "release"]
UNKNOWN = "?"
HOST = re.compile(r"[a-z0-9][a-z0-9-]*\Z")


def pair_sort(p):
    prof, host = p.split("@", 1)
    return (PROFILES.index(prof), host)


def parse_pairs(path, line, field):
    if field == "-":
        return set()
    out = set()
    for p in field.split(","):
        prof, _, host = p.partition("@")
        if prof not in PROFILES or not (host == UNKNOWN or HOST.match(host)):
            sys.exit(f"merge.py: {path}: bad provenance {p!r} in {line!r} (want <profile>@<host class>)")
        out.add(p)
    return out


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
            elif len(parts) in (3, 4) and parts[1] in TIERS:
                profs = set() if parts[2] == "-" else set(parts[2].split(","))
                if not profs <= set(PROFILES):
                    sys.exit(f"merge.py: {path}: unknown profile in {line!r}")
                if len(parts) == 3:
                    pairs = {f"{p}@{UNKNOWN}" for p in profs}
                else:
                    pairs = parse_pairs(path, line, parts[3])
                    if {p.split("@", 1)[0] for p in pairs} != profs:
                        sys.exit(f"merge.py: {path}: profiles and provenance disagree in {line!r}")
                out.append((parts[0], parts[1], pairs))
            else:
                sys.exit(f"merge.py: {path}: bad line {line!r} (want <key>\\t<tier>\\t<profiles>\\t<provenance>)")
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
    if len(sys.argv) != 5:
        sys.exit("usage: merge.py <old list lines> <new keys> <profile> <host class>")
    old_path, new_path, profile, host = sys.argv[1:5]
    if profile not in PROFILES:
        sys.exit(f"merge.py: unknown profile {profile!r} (want one of {PROFILES})")
    if not HOST.match(host):
        sys.exit(f"merge.py: bad host class {host!r} (want [a-z0-9-]+, e.g. intel-6-85)")
    this, unknown = f"{profile}@{host}", f"{profile}@{UNKNOWN}"
    old, new = read_old(old_path), read_new(new_path)
    tier = dict(new)
    pairs = {k: {this} for k in new}
    out = list(new)
    pos = {k: i for i, k in enumerate(out)}
    anchor = None  # the last old key already placed
    for k, t, ps in old:
        legacy = t is None
        others = ps - {this, unknown}
        if k in pos:
            # Traced now: keep the other pairs and the earlier of the tiers they agree on.
            if others and not legacy:
                pairs[k] |= others
                tier[k] = min(tier[k], t, key=TIERS.index)
            anchor = k
            continue
        others = ps - {this}
        if ps and not others:
            continue  # reached only by this profile on this host class, and no longer: stale
        i = pos[anchor] + 1 if anchor is not None else 0
        out.insert(i, k)
        pos = {k2: j for j, k2 in enumerate(out)}
        tier[k] = "extra" if legacy else t
        pairs[k] = others
        anchor = k
    for k in out:
        ps = sorted(pairs[k], key=pair_sort)
        profs = ",".join(x for x in PROFILES if any(p.startswith(x + "@") for p in ps)) or "-"
        print(f"{k}\t{tier[k]}\t{profs}\t{','.join(ps) or '-'}")


if __name__ == "__main__":
    main()
