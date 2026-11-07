#!/usr/bin/env python3
"""Compare wcprobe count tables: median net instructions (and loads) over classes per locale/entry/range.
usage: cmp.py before.tsv after.tsv glibc.tsv"""
import sys
from statistics import median


def load(p):
    d = {}
    for line in open(p):
        f = line.rstrip('\n').split('\t')
        if len(f) < 8 or f[1] == 'base-gross':
            continue
        try:
            d.setdefault((f[0], f[1], f[3]), []).append((float(f[4]), float(f[7])))
        except ValueError:
            pass
    return d


b, a, g = (load(p) for p in sys.argv[1:4])
ranges = ['ascii', 'latin1', 'cjk', 'bmpmix', 'astral', 'bmprnd']
print('locale entry: per range before/after/glibc instr (after loads); worst after-before delta')
for key in sorted({k[:2] for k in a}):
    loc, ent = key
    if not ent.startswith('isw'):
        continue
    cells, worst = [], -1e9
    for r in ranges:
        if (loc, ent, r) not in a:
            continue
        bi = median(x[0] for x in b.get((loc, ent, r), [(float('nan'), 0)]))
        ai = median(x[0] for x in a[(loc, ent, r)])
        al = median(x[1] for x in a[(loc, ent, r)])
        gi = median(x[0] for x in g.get((loc, ent, r), [(float('nan'), 0)]))
        worst = max(worst, max(x[0] for x in a[(loc, ent, r)]) - min(x[0] for x in b.get((loc, ent, r), [(0, 0)])))
        cells.append(f'{r} {bi:.0f}/{ai:.0f}/{gi:.0f} ({al:.0f})')
    print(f'{loc} {ent}: ' + '; '.join(cells) + f'; worst +{worst:.0f}')
