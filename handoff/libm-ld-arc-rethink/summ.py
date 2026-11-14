#!/usr/bin/env python3
"""Summarise run_probe.sh output: per-call counts = (count(n) - count(ident, 0 calls)) / n per bin, case, mode
(averaged over event groups for cycles/instructions), the fenced TSC lines (ticks and cycles via mb_imul), and
the latency microbenches (cycles per chained op). usage: summ.py <log>"""
import re
import sys
from collections import defaultdict

lines = open(sys.argv[1], errors='replace').read().splitlines()
base = {}
tab = defaultdict(lambda: defaultdict(list))
order = []
mb = {}
tsc = []
other = []
for ln in lines:
    w = ln.split()
    if not w:
        continue
    if w[0] == 'R' and len(w) >= 5:
        b, c, mode, n = w[1], w[2], w[3], int(w[4])
        ev = {k: float(v) for k, v in re.findall(r'(\S+?):u=([0-9.]+)', ln)}
        if c == 'ident' and n == 0:
            base[(b, mode)] = ev
            continue
        key = (b, c, mode)
        if key not in tab:
            order.append(key)
        bs = base.get((b, mode), {})
        for k, v in ev.items():
            tab[key][k].append((v - bs.get(k, 0.0)) / n)
        m = re.search(r'ticks/call=([0-9.]+)', ln)
        if m:
            tab[key]['ticks'].append(float(m.group(1)))
    elif w[0] == 'M':
        ev = {k: float(v) for k, v in re.findall(r'(\S+?):u=([0-9.]+)', ln)}
        n = int(w[2])
        ops = re.search(r'ops/iter=(\d+)', ln)
        tk = re.search(r'ticks/iter=([0-9.]+)', ln)
        mb[w[1]] = (ev.get('cycles', 0) / n, ev.get('instructions', 0) / n, int(ops.group(1)) if ops else 1,
                    float(tk.group(1)) if tk else 0.0, ev)
    elif len(w) > 1 and w[1] == 'tsc':
        tsc.append(ln)
    else:
        other.append(ln)

for o in other[:12]:
    print(o[:160])
tpc = 1.0
if 'mb_imul' in mb and mb['mb_imul'][0]:
    tpc = mb['mb_imul'][3] / mb['mb_imul'][0]
print(f'ticks per cycle (mb_imul) = {tpc:.3f}')
print('bin    case        mode   cyc    ins   IPC  | other events per call (mean over groups)')
for key in order:
    d = tab[key]
    avg = {k: sum(v) / len(v) for k, v in d.items()}
    cyc, ins = avg.get('cycles', 0), avg.get('instructions', 0)
    rest = ' '.join(f'{k.split(".")[-1] if k.count(".") else k}={v:.1f}' for k, v in avg.items()
                    if k not in ('cycles', 'instructions', 'ticks') and abs(v) >= 0.5)
    print(f'{key[0]:6} {key[1]:11} {key[2]:4} {cyc:6.0f} {ins:6.0f} {ins / cyc if cyc else 0:4.2f} | {rest}')
print('microbench: cycles per chained op (perf) / ticks per op / instructions per iter')
for k, (c, i, ops, tk, ev) in mb.items():
    print(f'  {k:12} {c / ops:7.2f} cyc/op  {tk / ops:7.2f} ticks/op  {i:6.1f} ins/iter  ops={ops}')
print('fenced TSC (ticks; cycles = ticks / tpc):')
for t in tsc:
    def cv(m):
        return f'{m.group(1)}={int(m.group(2)) / tpc:.0f}'
    print('  ' + re.sub(r'(\w+)=(\d+)', cv, t.split(' n=')[0] + ' |' + t.split('ticks', 1)[1])[:200])
