#!/usr/bin/env python3
"""COMPARE-ENTRY-RETHINK-2: table from a build.sh (count) or time2.sh (time) log.
usage: analyze2.py LOG [count-event, default instructions]
Time: median ns over the process rounds; next = Liella's own entry; S = SSE2 entry, A / B = AVX2 entry orders A / B,
P = `jmp *slot` to the CPU's entry; ratios to next (same binary) and next/glibc; last lines: ratio distributions."""
import sys, statistics as st, collections
log = sys.argv[1]
ev = sys.argv[2] if len(sys.argv) > 2 else "instructions"
d = collections.defaultdict(lambda: collections.defaultdict(list))
mode = "native"
for line in open(log, errors="replace"):
    w = line.split()
    if not w:
        continue
    if w[0] == "H":
        kv = dict(x.split("=", 1) for x in w[1:] if "=" in x)
        mode = "sse2" if kv.get("tunables", "-") != "-" else "native"
    elif w[0] == "R" and len(w) >= 6:
        _, var, fn, n, kind = w[:5]
        seen = set()
        for x in w[5:]:
            k, v = x.split("=")
            if k not in seen:
                seen.add(k)
                d[(mode, var, fn, n, kind)][k].append(float(v))
timing = any("ns" in v for v in d.values())
key = "ns" if timing else ev
def g(m, var, fn, n, kind):
    v = d.get((m, var, fn, n, kind), {}).get(key)
    return st.median(v) if v else None
rows = sorted({(k[3], k[4]) for k in d}, key=lambda r: (int(r[0]), r[1]))
dist = collections.defaultdict(list)
for m in sorted({k[0] for k in d}):
    for fam, libs, pre in (("memcmp", ["memcmp"], "p_memcmp_"), ("eq", ["bcmp", "__memcmpeq"], "p_memeq_")):
        for lib in libs:
            print(f"[{m}] {lib}: n kind | glibc {key} | next/glibc | S/next A/next B/next P/next | S/glibc A/glibc")
            for n, kind in rows:
                gl = g(m, "glibc", lib, n, kind)
                nx = g(m, "liella", lib, n, kind)
                if gl is None or nx is None:
                    continue
                vs = {c: g(m, "liella", pre + s, n, kind) for c, s in (("S", "sse2"), ("A", "avx2"), ("B", "avx2b"), ("P", "ptr"))}
                if timing:
                    r = lambda x, b: f"{x / b:.3f}" if x and b else "  -  "
                    for c, x in vs.items():
                        if x and nx:
                            dist[(m, lib, c)].append(x / nx)
                    print(f"  {n:>4} {kind:<5} | {gl:7.2f} | {r(nx, gl)} | " + " ".join(r(vs[c], nx) for c in "SABP")
                          + f" | {r(vs['S'], gl)} {r(vs['A'], gl)}")
                else:
                    f = lambda x: "-" if x is None else f"{x:.1f}"
                    print(f"  {n:>4} {kind:<5} | {f(gl)} | next {f(nx)} | " + " ".join(f"{c} {f(vs[c])}" for c in "SABP"))
for (m, lib, c), v in sorted(dist.items()):
    v.sort()
    print(f"DIST [{m}] {lib} {c}/next: n={len(v)} min {v[0]:.3f} p10 {v[len(v) // 10]:.3f} median {st.median(v):.3f}"
          f" p90 {v[(9 * len(v)) // 10]:.3f} max {v[-1]:.3f} rows>1.05: {sum(x > 1.05 for x in v)}")
