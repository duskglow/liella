#!/usr/bin/env python3
"""Proposed instruction limits from a bench run's counters.tsv (LIMITS-GEN).

    limits_from_counters.py COUNTERS.tsv [--headroom 2%|N] [--rows a,b] [--cpu MODEL] [--check [LIMITS.tsv]]

COUNTERS.tsv is target/liella/bench/counters.tsv of `cargo xtask bench --compare --counters` (columns: bench row
ns cycles upc instr  L-cycles G-cycles L-upc G-upc L-instr G-instr spread flags; the two instr columns are the
median Liella and glibc instructions per call). Output: lines in the exact format of bench/instr_limits.tsv
(`cpu bench row max_instr rationale`), one per selected row that has a Liella instruction count.

  limit = ceil(Liella instr + headroom), headroom = max(2 instr, PCT of the count) for `--headroom PCT%` (default
  2%), or exactly N instructions for `--headroom N`. --rows takes `row` or `bench/row` names (comma list).
  --cpu is the `model name` of /proc/cpuinfo (default: this machine's; `*` for every CPU).

--check [LIMITS.tsv] (default bench/instr_limits.tsv next to this script) diffs the proposal against the current
file instead of printing lines: LOOSE = current limit more than 5% above the proposal (candidate to tighten),
WOULD-FAIL = current limit below the Liella count (the bench gate fails it), NO-ROW = limit line whose row is
absent from the counters, NEW = counted row without a limit. Exit 1 when any WOULD-FAIL. Never edits a file.
"""
import math
import os
import sys

LOOSE_PCT = 5.0


def ceil_instr(x):
    return int(math.ceil(round(x, 6)))


def headroom(count, spec):
    if spec.endswith("%"):
        return max(2.0, count * float(spec[:-1]) / 100.0)
    return float(spec)


def limit_for(count, spec):
    return ceil_instr(count + headroom(count, spec))


def parse_counters(text):
    """[(bench, row, liella_instr, glibc_instr or None)] for rows with a Liella instruction count."""
    out = []
    for ln in text.splitlines():
        f = ln.split("\t")
        if len(f) < 12 or not f[0]:
            continue
        try:
            li = float(f[10])
        except ValueError:
            continue  # `-`: no counter for this row
        try:
            gi = float(f[11])
        except ValueError:
            gi = None
        out.append((f[0], f[1], li, gi))
    return out


def select(rows, names):
    if not names:
        return rows
    want = set(names)
    return [r for r in rows if r[1] in want or f"{r[0]}/{r[1]}" in want]


def propose(rows, cpu, spec):
    """[(cpu, bench, row, limit, rationale)]"""
    res = []
    for b, r, li, gi in rows:
        g = "" if gi is None else f" vs glibc {gi:.1f}"
        res.append((cpu, b, r, limit_for(li, spec), f"Liella {li:.1f}{g}; headroom {spec}"))
    return res


def parse_limits(text):
    out = []
    for ln in text.splitlines():
        if not ln.strip() or ln.startswith("#"):
            continue
        f = ln.split("\t")
        if len(f) >= 4:
            out.append((f[0], f[1], f[2], int(f[3])))
    return out


def check(rows, spec, limits, cpu):
    """[(kind, bench/row, text)] findings of the proposal against the limits that apply to cpu."""
    cur = {(b, r): m for c, b, r, m in limits if c in (cpu, "*")}
    seen, res = set(), []
    for b, r, li, _ in rows:
        key = (b, r)
        seen.add(key)
        prop = limit_for(li, spec)
        if key not in cur:
            res.append(("NEW", f"{b}/{r}", f"no limit; Liella {li:.1f}, proposed {prop}"))
            continue
        c = cur[key]
        if c < li:
            res.append(("WOULD-FAIL", f"{b}/{r}", f"limit {c} < Liella count {li:.1f}"))
        elif (c - prop) / prop * 100.0 > LOOSE_PCT:
            res.append(("LOOSE", f"{b}/{r}", f"limit {c}, Liella {li:.1f}, proposed {prop} ({(c - prop) / prop * 100.0:.1f}% looser)"))
    for key in sorted(cur):
        if key not in seen:
            res.append(("NO-ROW", f"{key[0]}/{key[1]}", f"limit {cur[key]} but the counters have no such row"))
    return res


def local_cpu():
    try:
        with open("/proc/cpuinfo", encoding="utf-8") as fh:
            for ln in fh:
                if ln.startswith("model name"):
                    return ln.split(":", 1)[1].strip()
    except OSError:
        pass
    return None


def main(argv):
    args, i = [], 0
    opt = {"headroom": "2%", "rows": [], "cpu": None, "check": None}
    while i < len(argv):
        a = argv[i]
        if a in ("--headroom", "--rows", "--cpu"):
            if i + 1 >= len(argv):
                sys.exit(f"{a} needs a value")
            v = argv[i + 1]
            opt[a[2:]] = v.split(",") if a == "--rows" else v
            i += 2
        elif a == "--check":
            nxt = argv[i + 1] if i + 1 < len(argv) else None
            if nxt and nxt.endswith(".tsv") and args:
                opt["check"], i = nxt, i + 2
            else:
                opt["check"], i = "", i + 1
        elif a in ("-h", "--help"):
            print(__doc__)
            return 0
        else:
            args.append(a)
            i += 1
    if len(args) != 1:
        sys.exit(__doc__)
    spec = opt["headroom"]
    try:
        float(spec[:-1] if spec.endswith("%") else spec)
    except ValueError:
        sys.exit(f"bad --headroom {spec!r} (want 2% or N)")
    cpu = opt["cpu"] or local_cpu() or sys.exit("--cpu needed (no /proc/cpuinfo)")
    with open(args[0], encoding="utf-8") as fh:
        rows = select(parse_counters(fh.read()), opt["rows"])
    if opt["check"] is None:
        for p in propose(rows, cpu, spec):
            print("\t".join(str(x) for x in p))
        return 0
    path = opt["check"] or os.path.join(os.path.dirname(os.path.abspath(__file__)), "instr_limits.tsv")
    with open(path, encoding="utf-8") as fh:
        limits = parse_limits(fh.read())
    res = check(rows, spec, limits, cpu)
    for kind, name, text in res:
        print(f"{kind:<10} {name}: {text}")
    n = {k: sum(1 for r in res if r[0] == k) for k in ("LOOSE", "WOULD-FAIL", "NEW", "NO-ROW")}
    print(f"# {len(rows)} counted rows, {n['LOOSE']} loose (>{LOOSE_PCT:.0f}%), {n['WOULD-FAIL']} would fail, "
          f"{n['NEW']} new, {n['NO-ROW']} without a counted row (cpu {cpu}, headroom {spec})")
    return 1 if n["WOULD-FAIL"] else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
