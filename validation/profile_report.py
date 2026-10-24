#!/usr/bin/env python3
"""Summarise validation/profile.sh's perf data as Markdown (for validation/profiles.md).

    validation/profile_report.py PROFILE_DIR LIELLA_SYMBOLS > report.md

For each workload: the timing rows, the share of samples in the C library (Liella: symbols
libc.a defines, Rust symbols of the library, and its C layer; glibc: libc.so.6 and ld.so), the
kernel and the program, and the ten hottest C-library functions of each build. Rust symbol
names are shortened to their path (a light reading of the v0 mangling, enough to name them).
"""
import re
import subprocess
import sys
from collections import defaultdict

prof, symfile = sys.argv[1], sys.argv[2]
liella_syms = set(open(symfile, encoding="utf-8").read().split())


def short(sym):
    """`_RNvCs8BLb1pX93Qe_10liella_mem7copy_u8` -> `liella_mem::copy_u8` (approximately)."""
    if not sym.startswith("_R"):
        return sym
    # Crate disambiguators (`Cs8BLb1pX93Qe_`) and impl disambiguators (`Ms2_`) carry no name.
    sym = re.sub(r"Cs[0-9A-Za-z]+_", "C", sym)
    sym = re.sub(r"Ms[0-9A-Za-z]*_", "M", sym)
    out = []
    i = 0
    while i < len(sym):
        m = re.match(r"(\d+)", sym[i:])
        if m and i > 0 and not sym[i - 1].isdigit():
            n = int(m.group(1))
            start = i + len(m.group(1))
            if start < len(sym) and sym[start] == "_":
                start += 1
            word = sym[start:start + n]
            if re.fullmatch(r"[A-Za-z_][A-Za-z0-9_]*", word or "-") and len(word) == n:
                out.append(word)
                i = start + n
                continue
        i += 1
    # Crate hashes (`Cs8BLb1pX93Qe_`) are skipped by the length check; generic arguments
    # repeat path segments, so only the first occurrence of each is kept.
    seen = []
    for w in out:
        if w not in seen:
            seen.append(w)
    return "::".join(seen) if seen else sym


def samples(data, glibc):
    cmd = ["perf", "report", "-q", "-i", data, "--no-children", "--sort", "dso,symbol", "--stdio"]
    text = subprocess.run(cmd, capture_output=True, text=True).stdout
    rows = []
    for line in text.splitlines():
        m = re.match(r"\s+([0-9.]+)%\s+(\S+)\s+\[(.)\]\s+(\S+)", line)
        if m:
            rows.append((float(m.group(1)), m.group(2), m.group(3), m.group(4)))
    share = defaultdict(float)
    libc = defaultdict(float)
    for pct, dso, kind, sym in rows:
        if kind == "k":
            share["kernel"] += pct
        elif glibc and (dso.startswith("libc.so") or dso.startswith("ld-linux")):
            share["libc"] += pct
            libc[sym] += pct
        elif not glibc and (sym in liella_syms or sym.startswith("_R") or sym.startswith("liella_")):
            share["libc"] += pct
            libc[short(sym)] += pct
        else:
            share["program"] += pct
    return share, sorted(libc.items(), key=lambda kv: -kv[1])[:10]


timings = defaultdict(dict)
for line in open(f"{prof}/timings.tsv", encoding="utf-8").read().splitlines()[1:]:
    f = line.split("\t")
    timings[f[0]][f[1]] = f
for name in timings:
    t = timings[name]
    print(f"### {name}\n")
    print("| build | CPU s (mean of 5) | stddev | wall s (one run) | load before | load after |")
    print("|---|---|---|---|---|---|")
    for b in ("glibc", "liella"):
        if b in t:
            f = t[b]
            print(f"| {b} | {f[2]} | {f[3]} | {f[6].replace('wall=', '')} | {f[4]} | {f[5]} |")
    try:
        g, l = float(t["glibc"][2]), float(t["liella"][2])
        print(f"\nLiella / glibc CPU time: **{l / g:.2f}**\n")
    except (KeyError, ValueError, ZeroDivisionError):
        print()
    for b, data, glibc in (("glibc", f"{prof}/{name}-glibc.data", True), ("liella", f"{prof}/{name}.data", False)):
        share, top = samples(data, glibc)
        print(f"{b}: C library {share['libc']:.1f}%, program {share['program']:.1f}%, kernel {share['kernel']:.1f}% of samples. "
              "Hottest C-library functions: " + ", ".join(f"`{s}` {p:.1f}%" for s, p in top) + ".\n")
