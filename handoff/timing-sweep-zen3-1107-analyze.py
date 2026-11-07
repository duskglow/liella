#!/usr/bin/env python3
"""TIMING-SWEEP-NATSUMI analyzer: absolute Liella/glibc ratios of one CPU class from `sweep run` tables.

usage: python3 -I analyze.py <nonlibm sweep.tsv> [<libm sweep.tsv> | -] <older results table for orientation>

`sweep compare` compares the same class over two sweeps; this sweep has no earlier Zen 3 sweep, so this prints, per
set (non-libm, libm): faster / parity / slower counts (the sweep tool's thresholds: faster < 0.95, slower > 1.05,
rows naming wordexp timed but not judged), the spread distribution, the flagged rows, the slowest 20 rows and the
full table. Percentiles use the sweep tool's rule (sorted[int(n * p)], capped at the last element).
The older table's summary counts are copied for orientation only (other CPU classes, other commit).
Produced results/timing-sweep-zen3-1107-table.md; the runs, host setup and measurement conditions are recorded in
handoff/timing-sweep-natsumi.md.
"""
import re
import statistics
import sys

FASTER, SLOWER = 0.95, 1.05
DEFERRED = ("wordexp",)


def num(s):
    return None if s in ("-", "") else float(s)


def load(path):
    rows = []
    with open(path) as f:
        head = f.readline().rstrip("\n").split("\t")
        for line in f:
            r = dict(zip(head, line.rstrip("\n").split("\t")))
            for k in ("instr", "cycles", "time", "spread"):
                r[k] = num(r[k])
            r["deferred"] = any(d in r["row"] for d in DEFERRED)
            rows.append(r)
    return rows


def pct(s, p):
    return s[min(int(len(s) * p), len(s) - 1)]


def verdict(t):
    return "faster" if t < FASTER else "slower" if t > SLOWER else "parity"


def f3(x):
    return "-" if x is None else f"{x:.3f}"


def report(name, rows):
    judged = [r for r in rows if not r["deferred"] and r["time"] is not None]
    deferred = [r for r in rows if r["deferred"]]
    for r in judged:
        # sweep.tsv prints 3 decimals and the tool judges the unrounded ratio: at a threshold its verdict stands.
        at_edge = min(abs(r["time"] - FASTER), abs(r["time"] - SLOWER)) <= 0.0005
        if verdict(r["time"]) != r["verdict"] and not at_edge:
            sys.exit(f"verdict mismatch {r['bench']}/{r['row']}: {r['time']} {r['verdict']}")
    c = {v: sum(1 for r in judged if r["verdict"] == v) for v in ("faster", "parity", "slower")}
    times = sorted(r["time"] for r in judged)
    spreads = sorted(r["spread"] for r in rows if r["spread"] is not None)
    instr = sorted(r["instr"] for r in judged if r["instr"] is not None)
    # A ratio printed 0.000 (< 0.0005) has no logarithm: it counts as faster but stays out of the geometric mean.
    zero = [r for r in judged if r["time"] <= 0]
    geo = statistics.geometric_mean([t for t in times if t > 0])
    out = [f"## {name}", ""]
    out.append("| rows judged | faster | parity | slower | time median | time p10 | time p90 | time geomean | "
               "spread median | spread p90 | spread max | instr median (counted rows) |")
    out.append("|---|---|---|---|---|---|---|---|---|---|---|---|")
    out.append(f"| {len(judged)} | {c['faster']} | {c['parity']} | {c['slower']} | {statistics.median(times):.3f} | "
               f"{pct(times, 0.1):.3f} | {pct(times, 0.9):.3f} | {geo:.3f} | {statistics.median(spreads):.3f} | "
               f"{pct(spreads, 0.9):.3f} | {spreads[-1]:.3f} | "
               f"{(statistics.median(instr) if instr else float('nan')):.3f} ({len(instr)}) |")
    out.append("")
    if zero:
        out.append("Rows with time printed 0.000 (< 0.0005; faster, left out of the geomean): "
                   + ", ".join(f"{r['bench']}/{r['row']}" for r in zero))
    flagged =[r for r in rows if r["flagged"] == "yes"]
    out.append(f"Rows flagged by the bench's per-host limit check: {len(flagged)}"
               + ("" if flagged else " (no limits exist for this CPU model; see the handoff)."))
    out += [f"- {r['bench']}/{r['row']} time {f3(r['time'])} limit {r['limit']} status {r['status']}" for r in flagged]
    if deferred:
        out.append(f"Deferred rows (timed, not judged): " + ", ".join(
            f"{r['bench']}/{r['row']} {f3(r['time'])}" for r in deferred))
    noisy = [r for r in judged if r["spread"] is not None and r["spread"] > 0.10 and r["verdict"] != "parity"
             and abs(r["time"] - 1) < r["spread"]]
    out.append(f"Judged rows whose verdict lies inside their own spread (spread > 0.10 and |time - 1| < spread): "
               f"{len(noisy)}" + ("" if not noisy else ": " + ", ".join(f"{r['bench']}/{r['row']}" for r in noisy)))
    out += ["", f"### {name}: slowest 20 rows", "",
            "| bench/row | time | instr | cycles | spread | rounds |", "|---|---|---|---|---|---|"]
    for r in sorted(judged, key=lambda r: -r["time"])[:20]:
        out.append(f"| {r['bench']}/{r['row']} | {f3(r['time'])} | {f3(r['instr'])} | {f3(r['cycles'])} | "
                   f"{f3(r['spread'])} | {r['rounds']} |")
    return out, judged


def table(name, rows):
    out = [f"### {name}: all rows", "", "| bench/row | instr | cycles | time | spread | rounds | verdict |",
           "|---|---|---|---|---|---|---|"]
    for r in sorted(rows, key=lambda r: (r["bench"], r["row"])):
        v = "deferred" if r["deferred"] else r["verdict"]
        out.append(f"| {r['bench']}/{r['row']} | {f3(r['instr'])} | {f3(r['cycles'])} | {f3(r['time'])} | "
                   f"{f3(r['spread'])} | {r['rounds']} | {v} |")
    return out


def orientation(path):
    out = []
    with open(path) as f:
        text = f.read()
    for m in re.finditer(r"^\| ((?:Ivy Bridge|N100) now \((\d+) judged rows\)) \| (\d+) \| (\d+) \| (\d+) \| ([\d.]+) \|",
                         text, re.M):
        out.append((m.group(1).replace(" now ", " 1106E "), int(m.group(3)), int(m.group(4)), int(m.group(5)), m.group(6)))
    if not out:
        sys.exit(f"no summary rows in {path}")
    return out


def main():
    if len(sys.argv) != 4:
        sys.exit(__doc__)
    sets = [("Zen 3, non-libm rows", load(sys.argv[1]))]
    if sys.argv[2] != "-":
        sets.append(("Zen 3, libm rows", load(sys.argv[2])))
    lines, tables = [], []
    for name, rows in sets:
        rep, _ = report(name, rows)
        lines += rep + [""]
        tables += table(name, rows) + [""]
    lines += [f"## Orientation only: other CPU classes, other commit ({sys.argv[3]})", "",
              "| class (non-libm rows) | faster | parity | slower | spread median |", "|---|---|---|---|---|"]
    lines += [f"| {o[0]} | {o[1]} | {o[2]} | {o[3]} | {o[4]} |" for o in orientation(sys.argv[3])]
    print("\n".join(lines + [""] + tables))


main()
