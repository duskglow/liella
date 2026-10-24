#!/usr/bin/env python3
"""Sum MEMCMP-SIZE-MIX histogram dumps (one 25680-byte record per process: 5 entries x {head equal or n < 8,
head differs} x 321 buckets of u64; bucket n for n <= 256, 257 + floor(log2 n) above) and print calls per
band. Usage: mcs_hist.py FILE.bin... (one file per workload; a TOTAL table follows)."""
import os
import struct
import sys

KINDS = ["memcmp", "bcmp", "__memcmpeq", "int-compare", "int-equal"]
BANDS = [(0, 0), (1, 7), (8, 15), (16, 32), (33, 64), (65, 128), (129, 2048), (2049, 1 << 62)]
REC = 5 * 642 * 8


def bucket_band(b):
    lo = b if b <= 256 else 1 << (b - 257)
    for i, (a, z) in enumerate(BANDS):
        if a <= lo <= z:
            return i
    return len(BANDS) - 1


def load(path):
    data = open(path, "rb").read()
    tot = [0] * (5 * 642)
    for off in range(0, len(data) - REC + 1, REC):
        vals = struct.unpack_from("<%dQ" % (5 * 642), data, off)
        tot = [x + y for x, y in zip(tot, vals)]
    return tot, len(data) // REC


def table(name, tot, nrec):
    names = ["%d-%d" % b if b[1] < 1 << 62 else ">2048" for b in BANDS]
    print("== %s (%d process dumps)" % (name, nrec))
    print("%-12s %10s " % ("entry", "calls") + " ".join("%11s" % n for n in names) + "   hd-diff(>=8)")
    for k, kn in enumerate(KINDS):
        band = [0] * len(BANDS)
        diff = 0
        for h in (0, 1):
            for b in range(321):
                c = tot[k * 642 + h * 321 + b]
                band[bucket_band(b)] += c
                if h:
                    diff += c
        s = sum(band)
        if not s:
            continue
        ge8 = sum(band[2:])
        print("%-12s %10d " % (kn, s) + " ".join("%5.1f%%%5s" % (100.0 * c / s, "") for c in band)
              + "   %5.1f%%" % (100.0 * diff / ge8 if ge8 else 0))


grand = [0] * (5 * 642)
gn = 0
for p in sys.argv[1:]:
    t, n = load(p)
    table(os.path.basename(p)[:-4], t, n)
    grand = [x + y for x, y in zip(grand, t)]
    gn += n
table("TOTAL", grand, gn)
