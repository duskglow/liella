#!/usr/bin/env python3
"""Compare two binding tables (tools/binding_table.sh output): binding_diff.py glibc.tsv liella.tsv
Prints counts and, with -v, the differing symbols (name, version, glibc bind, liella bind)."""
import sys


def load(p):
    d = {}
    with open(p, encoding="utf-8") as f:
        for line in f:
            n, v, b = line.rstrip("\n").split("\t")
            d[(n, v)] = b
    return d


g, l = load(sys.argv[1]), load(sys.argv[2])
diff = sorted((k, g[k], l[k]) for k in g if k in l and g[k] != l[k])
cnt = {}
for _, a, b in diff:
    cnt[(a, b)] = cnt.get((a, b), 0) + 1
print("common:", sum(1 for k in g if k in l), "differ:", len(diff), cnt)
if "-v" in sys.argv:
    for (n, v), a, b in diff:
        print(n, v, a, b, sep="\t")
