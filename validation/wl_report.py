#!/usr/bin/env python3
"""Workload gate, step 3: the table and the verdict.

    wl_report.py NEW BASE RUN.tsv [RETRY.tsv] [--host H] [--thr wl_thresholds.tsv] [--known wl_known_diffs.tsv]

RUN.tsv/RETRY.tsv come from wl_run.sh with the variants glibc, BASE and NEW (RETRY rows
replace the same workload's rows of RUN: the gate re-measures a workload that failed once).

OWNER RULE (2026-10-10): the gate fails ONLY on deterministic counters and on unexplained
output; wall time is measured and reported (the "time" column, advisory) but never a verdict.
FAIL (counter): NEW's user instructions (perf instructions:u of the run and its children),
page faults, or a syscall count (strace -c -f; total and per class rw/open/fstat/stat/dir/
mem/proc/other) exceed BASE's by more than the band
    band = max(0.5 %, 2 x the spread seen in A/A runs)       (wl_thresholds.tsv, per host/workload/metric)
and by more than an absolute floor (3 calls, 8 page faults) so a handful of calls on a small
count does not trip it. A decrease never fails.
FAIL (output): NEW's output differs from glibc's and the differing lines' fingerprint
(count:sha1, made by wl_run.sh) is not in wl_known_diffs.tsv for that workload (a known diff
whose fingerprint changes fails). Also FAIL: missing rows.
Exit 0 on WL-GATE PASS, 1 on FAIL. The FAIL line lists "workload(metric)" items.
"""
import csv
import math
import os
import sys

args = sys.argv[1:]
opts = {}
pos = []
i = 0
while i < len(args):
    if args[i] in ("--host", "--thr", "--known"):
        opts[args[i]] = args[i + 1]
        i += 2
    else:
        pos.append(args[i])
        i += 1
new, base, run = pos[:3]
retry = pos[3] if len(pos) > 3 else None
host = opts.get("--host", "")
here = os.path.dirname(os.path.abspath(__file__))
thr_path = opts.get("--thr", os.path.join(here, "wl_thresholds.tsv"))
known_path = opts.get("--known", os.path.join(here, "wl_known_diffs.tsv"))

METRICS = ["instr", "pf", "sys_total", "sys_rw", "sys_open", "sys_fstat", "sys_stat", "sys_dir", "sys_mem", "sys_proc", "sys_other"]
FLOOR = {"instr": 0, "pf": 8}  # absolute floors; syscall classes: 3
DEFAULT_THR = 0.5


def load(path):
    with open(path, encoding="utf-8", newline="") as f:
        return list(csv.DictReader((ln for ln in f if not ln.startswith("#")), delimiter="\t"))


thr = {}
if os.path.exists(thr_path):
    for r in load(thr_path):
        if r["host"] in (host, "*"):
            thr[(r["workload"], r["metric"])] = float(r["thr_pct"])
known = {}
if os.path.exists(known_path):
    for r in load(known_path):
        known[(r["workload"], r["fingerprint"])] = r["reason"]

rows = {}
retried = set()
for r in load(run):
    rows.setdefault(r["workload"], {})[r["variant"]] = r
if retry:
    for r in load(retry):
        if r["workload"] not in retried:
            rows[r["workload"]] = {}
            retried.add(r["workload"])
        rows[r["workload"]][r["variant"]] = r


def num(r, k):
    try:
        return float(r[k])
    except (KeyError, ValueError, TypeError):
        return None


print("counters = Liella user instructions / page faults / syscalls, NEW vs BASE (the gate; syscalls = the deterministic ones: futex and, for a run that created threads, the mmap/munmap/brk/mprotect/madvise family are left out, WLRUN-FUTEX); futex = futex calls NEW vs BASE (advisory: waits depend on timing, never a verdict); time = CPU seconds, cycles = user+kernel cycles NEW vs BASE (cyc u / cyc k = the user-only and kernel-only part, WLRUN-CYCLES-UK; '-' on a run file without them), GHz = NEW/BASE clock (all three advisory, never a verdict)")
print(f"{'workload':<13}{'instr n/g':>10}{'instr':>8}{'pf':>8}{'syscalls':>9}{'futex':>8}{'time':>8}{'cycles':>8}{'cyc u':>8}{'cyc k':>8}{'GHz n/b':>11}  verdict")
fails = []
ratios = []
for name, v in rows.items():
    try:
        g, b, n = v["glibc"], v[base], v[new]
    except KeyError:
        print(f"{name:<13} missing rows")
        fails.append(name + "(missing)")
        continue
    bad = []
    pct = {}
    for m in METRICS:
        bv, nv = num(b, m), num(n, m)
        if bv is None or nv is None:
            if m in ("instr", "pf"):
                bad.append(f"{m}:missing")
            continue
        d = nv - bv
        pct[m] = (d / bv * 100) if bv else (0.0 if d == 0 else 100.0)
        band = max(DEFAULT_THR, thr.get((name, m), 0.0))
        if d > FLOOR.get(m, 3) and pct[m] > band:
            bad.append(f"{m} {pct[m]:+.2f}%>{band:.2f}%")
    verdict = "ok"
    if bad:
        verdict = "COUNTERS " + "; ".join(bad)
        fails += [f"{name}({x.split()[0].split(':')[0]})" for x in bad]
    dfp = n.get("dfp", "-")
    if n["md5"] != g["md5"]:
        key = (name, dfp)
        if key in known:
            verdict += f" OUTPUT-DIFF known ({known[key]})"
        else:
            verdict += f" OUTPUT-DIFF unexplained (fingerprint {dfp})"
            fails.append(name + "(output)")
    if name in retried:
        verdict += " (retried)"
    gi, ni = num(g, "instr"), num(n, "instr")
    if gi and ni:
        ratios.append(ni / gi)
    fb, fn = num(b, "sys_futex"), num(n, "sys_futex")
    fchg = f"{(fn / fb - 1) * 100:+.1f}%" if fb and fn is not None else "-"
    tb, tn = num(b, "cpu_s"), num(n, "cpu_s")
    tchg = f"{(tn / tb - 1) * 100:+.1f}%" if tb and tn is not None else "-"
    # FALLBACK-CYCLES (advisory, never a verdict): cycles NEW vs BASE and each side's clock (cycles / CPU time); on the
    # bimodal-clock fallback hosts (nico, kotori) the time column is only meaningful next to the GHz.
    cb, cn = num(b, "cycles"), num(n, "cycles")
    cchg = f"{(cn / cb - 1) * 100:+.1f}%" if cb and cn is not None else "-"
    def cchg_of(k):
        x, y = num(b, k), num(n, k)
        return f"{(y / x - 1) * 100:+.1f}%" if x and y is not None else "-"
    cu, ck = cchg_of("cycles_u"), cchg_of("cycles_k")
    ghz = f"{cn / tn / 1e9:.2f}/{cb / tb / 1e9:.2f}" if cb and cn and tb and tn else "-"
    print(f"{name:<13}{(ni / gi if gi and ni else float('nan')):>10.3f}{pct.get('instr', float('nan')):>+7.2f}%{pct.get('pf', float('nan')):>+7.2f}%{pct.get('sys_total', float('nan')):>+8.2f}%{fchg:>8}{tchg:>8}{cchg:>8}{cu:>8}{ck:>8}{ghz:>11}  {verdict}")
if ratios:
    print(f"geometric mean instructions new/glibc over {len(ratios)} workloads: {math.exp(sum(map(math.log, ratios)) / len(ratios)):.3f}")
if fails:
    print("WL-GATE FAIL: " + ", ".join(dict.fromkeys(fails)))
    sys.exit(1)
print("WL-GATE PASS")
