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
`regen.sh --replace` starts from an empty list.

`--startup-head <startup-text.keys>` (regen.sh, ruling #4417, D-X.63): every key of that file the
merged list holds takes tier `startup`, and the start-up tier moves to the head of the list (both
parts keep their merged order). The start-up text is then contiguous: an aligned hot-text cluster
can never sit between two start-up keys and push the rest into another 64 KiB window
(STARTUP-TEXT-WINDOWS, D-X.49). Keys of that file the list lacks are not added (list order only).
`--alias <file>` (`keys.py --aliases` lines, `<symbol key>\t<section key>`; LAYOUT-TEXT-PIN-6, #4467):
a start-up key the list lacks that is a label inside a listed section (an assembly entry label, an
alias) brings that section's key to the head instead.
`--dropped <file>` (LAYOUT-TEXT-PIN-9, ruling #4539): writes the old keys the merge dropped
(`<key>\t<tier>\t<old provenance>`, empty file if none); each is also printed to stderr. regen.sh
refuses a list that drops a key unless given --allow-drop.
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
    if len(args) != 4:
        sys.exit("usage: merge.py <old list lines> <new keys> <profile> <host class> [--startup-head <startup-text.keys>] [--alias <file>] [--dropped <file>]")
    old_path, new_path, profile, host = args
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
    dropped = []
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
            dropped.append(f"{k}\t{t}\t{','.join(sorted(ps, key=pair_sort))}")
            continue  # reached only by this profile on this host class, and no longer: stale
        i = pos[anchor] + 1 if anchor is not None else 0
        out.insert(i, k)
        pos = {k2: j for j, k2 in enumerate(out)}
        tier[k] = "extra" if legacy else t
        pairs[k] = others
        anchor = k
    if head is not None:
        head |= {alias[k] for k in head if k not in pos and k in alias}
        for k in out:
            if k in head:
                tier[k] = "startup"
        out = [k for k in out if tier[k] == "startup"] + [k for k in out if tier[k] != "startup"]
    for k in out:
        ps = sorted(pairs[k], key=pair_sort)
        profs = ",".join(x for x in PROFILES if any(p.startswith(x + "@") for p in ps)) or "-"
        print(f"{k}\t{tier[k]}\t{profs}\t{','.join(ps) or '-'}")
    # LAYOUT-TEXT-PIN-9 (ruling #4539): a dropped key is never silent. Every one goes to stderr,
    # and to the --dropped file, where regen.sh refuses the new list unless told --allow-drop.
    for d in dropped:
        print(f"merge.py: DROPPED (no {this} trace hit it, no other pair keeps it): {d}", file=sys.stderr)
    if dropped_path is not None:
        with open(dropped_path, "w") as f:
            f.writelines(d + "\n" for d in dropped)


if __name__ == "__main__":
    main()
