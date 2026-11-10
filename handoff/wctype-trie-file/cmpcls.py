#!/usr/bin/env python3
"""Per-class before/after/glibc net instructions; prints every row where after > before.
usage: cmpcls.py before.tsv after.tsv glibc.tsv"""
import sys


def load(p):
    d = {}
    for line in open(p):
        f = line.rstrip('\n').split('\t')
        if len(f) < 8 or f[1] == 'base-gross':
            continue
        try:
            d[(f[0], f[1], f[2], f[3])] = float(f[4])
        except ValueError:
            pass
    return d


b, a, g = (load(p) for p in sys.argv[1:4])
worse = better = same = 0
for k in sorted(a):
    if k not in b:
        print('new', k, a[k])
        continue
    dlt = a[k] - b[k]
    if dlt > 0.5:
        worse += 1
        print('WORSE', '\t'.join(k), f'{b[k]:.0f} -> {a[k]:.0f} (glibc {g.get(k, float("nan")):.0f})')
    elif dlt < -0.5:
        better += 1
    else:
        same += 1
print(f'rows: worse {worse}, better {better}, same {same}')
