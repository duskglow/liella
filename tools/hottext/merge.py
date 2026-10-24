#!/usr/bin/env python3
"""Merge two ordered hot-text key lists (HOT-ORDER): merge.py <old keys> <new keys> > merged.

The new trace's first-touch order wins. An old key the new trace did not hit (the other build
profile's inlining, a probe that changed) goes right after the nearest key that preceded it in the
old list, or first if none did, so a union over both profiles keeps each profile's touch order.
"""
import sys


def read(path):
    with open(path) as f:
        return [l.strip() for l in f if l.strip() and not l.startswith("#")]


old, new = read(sys.argv[1]), read(sys.argv[2])
out = list(dict.fromkeys(new))
pos = {k: i for i, k in enumerate(out)}
anchor = None  # the last old key already placed
for k in old:
    if k in pos:
        anchor = k
        continue
    i = pos[anchor] + 1 if anchor is not None else 0
    out.insert(i, k)
    pos = {k2: j for j, k2 in enumerate(out)}
    anchor = k
for k in out:
    print(k)
