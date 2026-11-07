#!/usr/bin/env python3
"""COMPARE-HEAD-RETHINK: compact table from harness logs (count or time mode).
usage: analyze.py LOG [mode-filter avx2|sse2] [events, default instructions,r20c4,r010e,cycles]
Count: in-function per call (null row subtracted). Time: median ns over rounds/processes, ratios to glibc."""
import sys, statistics, collections
log = sys.argv[1]
modef = sys.argv[2] if len(sys.argv) > 2 else None
evs = (sys.argv[3] if len(sys.argv) > 3 else "instructions,r20c4,r010e,cycles").split(",")
data = collections.defaultdict(lambda: collections.defaultdict(list))
mode = "?"
for line in open(log, errors="replace"):
    w = line.split()
    if not w:
        continue
    if w[0] == "H":
        kv = dict(x.split("=", 1) for x in w[1:] if "=" in x)
        mode = "sse2" if kv.get("tunables", "-") != "-" else "avx2"
    elif w[0] == "R" and len(w) >= 6:
        _, var, fn, n, kind = w[:5]
        seen = set()
        for x in w[5:]:
            k, v = x.split("=")
            if k in seen:
                continue
            seen.add(k)
            data[(mode, var, fn, n, kind)][k].append(float(v))
def get(mode, var, fn, n, kind, ev):
    d = data.get((mode, var, fn, n, kind))
    if not d or ev not in d:
        return None
    return statistics.median(d[ev])
ROWS_CMP = [("16", "eq"), ("16", "head"), ("64", "eq"), ("64", "head"), ("96", "diff8"), ("127", "diff8"),
            ("128", "eq"), ("128", "head"), ("128", "tail"), ("144", "eq"), ("256", "eq"), ("1024", "head"),
            ("1024", "eq"), ("4096", "eq"), ("0", "mix16"), ("0", "mix64")]
ROWS_EQ = [("8", "eq"), ("16", "eq"), ("32", "eq"), ("64", "eq"), ("128", "eq"), ("128", "head"), ("128", "tail"),
           ("1024", "eq"), ("1024", "head"), ("0", "mix16"), ("0", "mix64")]
modes = sorted({k[0] for k in data if modef is None or k[0] == modef})
timing = any("ns" in d for d in data.values())
for m in modes:
    for fam, rows, gl, li, pr in (("memcmp", ROWS_CMP, "memcmp", "memcmp", "proto_memcmp"),
                                  ("__memcmpeq", ROWS_EQ, "__memcmpeq", "__memcmpeq", "proto_memeq")):
        if timing:
            print(f"[{m}] {fam}: n kind | glibc ns | liella/glibc | proto/glibc (proto in glibc binary)")
        else:
            print(f"[{m}] {fam}: n kind | glibc {'/'.join(evs)} | liella | proto")
        for n, kind in rows:
            if timing:
                g = get(m, "glibc", gl, n, kind, "ns"); l = get(m, "liella", li, n, kind, "ns")
                p = get(m, "liella", pr, n, kind, "ns"); pg = get(m, "glibc", pr, n, kind, "ns")
                if g:
                    f = lambda x: f"{x / g:.3f}" if x else "-"
                    print(f"  {n:>4} {kind:<5} | {g:7.2f} | {f(l)} | {f(p)} ({f(pg)})")
            else:
                cols = []
                for var, fn in (("glibc", gl), ("liella", li), ("liella", pr)):
                    vals = [get(m, var, fn, n, kind, e) for e in evs]
                    cols.append("/".join("-" if v is None else f"{v:.1f}" for v in vals))
                print(f"  {n:>4} {kind:<5} | " + " | ".join(cols))
