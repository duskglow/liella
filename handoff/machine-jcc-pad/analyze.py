#!/usr/bin/env python3
"""Summarise count.sh output: per row, Liella before/after and glibc: MITE uops/call, cycles/call, instr/call."""
import sys
from collections import defaultdict

d = defaultdict(dict)
for line in open(sys.argv[1]):
    w = line.split()
    if len(w) < 11 or w[0] != "ctr":
        continue
    v, exe, mode, row = w[1], w[2], w[3], w[4]
    instr, cyc, uops = float(w[5]), float(w[6]), float(w[10])
    who = "glibc" if ("host" in exe or "glibc" in exe) else "liella"
    key = (exe.split(".")[0], row)
    if mode == "mite":
        d[key][(v, who, "mite")] = uops
    else:
        d[key][(v, who, "cyc")] = cyc
        d[key][(v, who, "ins")] = instr
        d[key][(v, who, "uops")] = uops

def g(r, *k):
    x = r.get(k)
    return "-" if x is None else f"{x:.1f}"

print(f"{'bench/row':40} {'miteB':>6} {'miteA':>6} {'miteG':>6} {'cycB':>6} {'cycA':>6} {'cycG':>6} {'insB':>6} {'insA':>6}")
tot = defaultdict(float)
n = 0
worse = []
for key in sorted(d):
    r = d[key]
    print(f"{key[0][7:] + '/' + key[1]:40.40} {g(r,'before','liella','mite'):>6} {g(r,'after','liella','mite'):>6} {g(r,'after','glibc','mite'):>6} "
          f"{g(r,'before','liella','cyc'):>6} {g(r,'after','liella','cyc'):>6} {g(r,'after','glibc','cyc'):>6} "
          f"{g(r,'before','liella','ins'):>6} {g(r,'after','liella','ins'):>6}")
    try:
        mb, ma = r[("before", "liella", "mite")], r[("after", "liella", "mite")]
        cb, ca = r[("before", "liella", "cyc")], r[("after", "liella", "cyc")]
        ib, ia = r[("before", "liella", "ins")], r[("after", "liella", "ins")]
    except KeyError:
        continue
    n += 1
    tot["mb"] += mb; tot["ma"] += ma; tot["cb"] += cb; tot["ca"] += ca
    if ia > ib + 0.05:
        worse.append(f"instr {key[1]} {ib:.2f}->{ia:.2f}")
    if ma > mb + 0.5:
        worse.append(f"mite {key[1]} {mb:.1f}->{ma:.1f}")
print(f"rows={n} sum mite before={tot['mb']:.1f} after={tot['ma']:.1f}; sum cycles before={tot['cb']:.1f} after={tot['ca']:.1f}")
print("rows up:", "; ".join(worse) if worse else "none")
