"""Summarize measure.tsv (cargo xtask bench --compare --v3 --sorted --counters) for MEASURE-1.
Usage: python summarize.py measure.tsv [budgets.tsv]  -> prints parity changes and gap split."""
import math
import sys
from collections import defaultdict

rows = []
with open(sys.argv[1], encoding="utf-8") as f:
    head = f.readline().rstrip("\n").split("\t")
    for line in f:
        rows.append(dict(zip(head, line.rstrip("\n").split("\t"))))

ratio_rows = None
if len(sys.argv) > 2:
    ratio_rows = set()
    for line in open(sys.argv[2], encoding="utf-8"):
        p = line.rstrip("\n").split("\t")
        if len(p) > 2 and p[2] == "ratio_host":
            ratio_rows.add((p[0], p[1]))


def num(r, k):
    try:
        v = float(r.get(k, "-"))
        return v if v > 0 else None
    except ValueError:
        return None


fam = lambda r: r["row"].split("_")[0]
print(f"rows measured: {len(rows)}" + (f"; ratio_host rows in budgets: {len(ratio_rows)}" if ratio_rows else ""))
sel = [r for r in rows if ratio_rows is None or (r["bench"], r["row"]) in ratio_rows]
print(f"rows considered: {len(sel)}")

above_dyn = above_ss = 0
changed = []
split = defaultdict(lambda: [0, 0, []])  # family -> [instr-dominated, ipc-dominated, rows]
layout = []
v3 = []
glibc_sd = []
for r in sel:
    ls, gs = num(r, "liella_static_ratio"), num(r, "glibc_static_ratio")
    if ls is None:
        continue
    ss = ls / gs if gs else None
    above_dyn += ls > 1.0
    if ss is not None:
        above_ss += ss > 1.0
        if (ls > 1.0) != (ss > 1.0):
            changed.append((r["bench"], r["row"], ls, ss))
        glibc_sd.append(gs)
    srt = num(r, "sorted_ratio")
    if srt:
        layout.append((abs(math.log(srt / ls)), r["row"], ls, srt))
    vv = num(r, "v3_ratio")
    if vv:
        v3.append((vv / ls, r["row"], ls, vv))
    # Gap split on the like-for-like static pair: time ~ instructions / IPC.
    ir, pr = num(r, "instr_ratio_ss"), num(r, "ipc_ratio_ss")
    if ss and ss > 1.0 and ir and pr:
        li, lp = math.log(ir), -math.log(pr)
        s = split[fam(r)]
        s[0 if li >= lp else 1] += 1
        s[2].append(f"{r['row']} t{ss:.2f} i{ir:.2f} ipc{pr:.2f}")

print(f"above parity: {above_dyn} vs glibc dynamic (current pairing); {above_ss} vs glibc static (like-for-like)")
print("== parity status CHANGES under the static-static pairing (row: Liella/glibc-dyn -> Liella/glibc-static)")
for b, row, ls, ss in sorted(changed, key=lambda x: x[3] - x[2]):
    print(f"  {b}/{row}: {ls:.3f} -> {ss:.3f}")
glibc_sd.sort()
if glibc_sd:
    print(f"== glibc static/dynamic: median {glibc_sd[len(glibc_sd)//2]:.3f}, min {glibc_sd[0]:.3f}, max {glibc_sd[-1]:.3f}")
print("== gap split (static pair, rows above parity): family instr-dominated / IPC-dominated")
for f_, (a, b, ex) in sorted(split.items(), key=lambda x: -(x[1][0] + x[1][1])):
    print(f"  {f_:<12} {a:>2} / {b:>2}   e.g. {'; '.join(ex[:2])}")
layout.sort(reverse=True)
if layout:
    d = sorted(x[0] for x in layout)
    print(f"== layout (--sort-section=name vs default): median |delta| {math.exp(d[len(d)//2])-1:.1%}, p90 {math.exp(d[int(len(d)*0.9)])-1:.1%}, max {math.exp(d[-1])-1:.1%}")
    for x in layout[:5]:
        print(f"  {x[1]}: {x[2]:.3f} vs {x[3]:.3f}")
v3.sort()
if v3:
    g = sorted(x[0] for x in v3)
    print(f"== v3/baseline time: median {g[len(g)//2]:.3f}, best {g[0]:.3f}, worst {g[-1]:.3f}")
    for x in v3[:6]:
        print(f"  {x[1]}: {x[2]:.3f} -> {x[3]:.3f}")
