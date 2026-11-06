#!/usr/bin/python3
# analyze.py <out-dir>: <out-dir>/{before,after}-<i>/{rounds,counters}.tsv from run.sh.
# Per row: Liella/glibc time ratio per side (median of per-round ratios over all alternations), the
# after/before ratio of Liella's ns (median of per-round ratios, round j of run i paired), glibc's own
# after/before drift (should be ~1.00), spread, and the instruction ratio (Liella/glibc) per side from counters.tsv.
import csv, glob, os, statistics as st, sys, collections

out = sys.argv[1]

def runs(side):
    return sorted(glob.glob(os.path.join(out, side + "-*")), key=lambda p: int(p.rsplit("-", 1)[1]))

def rounds(d):
    r = collections.defaultdict(list)
    p = os.path.join(d, "rounds.tsv")
    if not os.path.exists(p):
        return r
    for x in csv.DictReader(open(p), delimiter="\t"):
        try:
            r[x["row"]].append((float(x["liella_ns"]), float(x["host_ns"])))
        except (KeyError, ValueError):
            pass
    return r

def counters(d):
    p = os.path.join(d, "counters.tsv")
    if not os.path.exists(p):
        return {}, []
    rows = list(csv.DictReader(open(p), delimiter="\t"))
    cols = [c for c in (rows[0].keys() if rows else []) if "ins" in c.lower()]
    return {x["row"]: x for x in rows}, cols

B = [rounds(d) for d in runs("before")]
A = [rounds(d) for d in runs("after")]
cb, cols = counters(runs("before")[-1]) if runs("before") else ({}, [])
ca, _ = counters(runs("after")[-1]) if runs("after") else ({}, [])
print("runs: before", len(B), "after", len(A), "instr columns:", cols)
names = sorted(set().union(*[set(x) for x in B + A]))

def med(v):
    return st.median(v) if v else float("nan")

def spread(v):
    return (max(v) - min(v)) / st.median(v) if v else float("nan")

print("| row | L/g time before | L/g time after | L after/before | glibc after/before | spread L after/before | instr L/g before | instr L/g after | cycles L/g before | cycles L/g after | L instr/op before->after |")
print("|---|---|---|---|---|---|---|---|---|---|---|")
for n in names:
    rb = [l / h for x in B for (l, h) in x.get(n, [])]
    ra = [l / h for x in A for (l, h) in x.get(n, [])]
    la, ga = [], []
    for xb, xa in zip(B, A):
        for (lb, hb), (l2, h2) in zip(xb.get(n, []), xa.get(n, [])):
            la.append(l2 / lb)
            ga.append(h2 / hb)
    def cm(side, col):
        v = []
        for d in runs(side):
            c, _ = counters(d)
            try:
                v.append(float(c[n][col]))
            except (KeyError, ValueError):
                pass
        return f"{med(v):.3f}" if v else "-"
    print(f"| {n} | {med(rb):.3f} | {med(ra):.3f} | {med(la):.3f} | {med(ga):.3f} | {spread(la):.3f} | {cm('before','instr_ratio')} | {cm('after','instr_ratio')} | {cm('before','cycles_ratio')} | {cm('after','cycles_ratio')} | {cm('before','liella_instr')}->{cm('after','liella_instr')} |")
