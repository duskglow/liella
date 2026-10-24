#!/usr/bin/env python3
"""Derive the gate's counter thresholds from an A/A run.

    wl_aa.py HOST AA.tsv... > wl_thresholds.tsv.new     (merge into validation/wl_thresholds.tsv)

AA.tsv = wl_run.sh output for ONE kit measured under several labels (kit kit@2 kit@3 ...).
Per workload and metric: spread = (max - min) / mean over the repeats, in percent;
thr_pct = max(2 x spread, 0.5). The gate (wl_report.py) uses the larger of this and 0.5.
Prints the table (host workload metric spread_pct thr_pct) on stdout.
"""
import csv
import sys

M = ["instr", "pf", "sys_total", "sys_rw", "sys_open", "sys_fstat", "sys_stat", "sys_dir", "sys_mem", "sys_proc", "sys_other"]
host = sys.argv[1]
vals = {}
for path in sys.argv[2:]:
    with open(path, encoding="utf-8", newline="") as f:
        for r in csv.DictReader(f, delimiter="\t"):
            for m in M:
                try:
                    vals.setdefault((r["workload"], m), []).append(float(r[m]))
                except ValueError:
                    pass
print("host\tworkload\tmetric\tspread_pct\tthr_pct")
for (w, m), xs in vals.items():
    if len(xs) < 3:
        continue
    mean = sum(xs) / len(xs)
    spread = (max(xs) - min(xs)) / mean * 100 if mean else 0.0
    print(f"{host}\t{w}\t{m}\t{spread:.3f}\t{max(2 * spread, 0.5):.2f}")
