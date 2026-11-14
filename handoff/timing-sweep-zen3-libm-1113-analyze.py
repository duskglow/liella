#!/usr/bin/env python3
"""TIMING-SWEEP-ZEN3-LIBM analyzer: absolute Liella/glibc libm ratios on Zen 3, set beside the 11-07 Zen 3 libm sweep.

usage: python3 -I analyze.py <libm sweep.tsv> <11-07 Zen 3 results table> <commit of this run> <commit of 11-07>

Prints faster / parity / slower counts (the sweep tool's thresholds: faster < 0.95, slower > 1.05), the spread
distribution, flagged rows, the slowest 20 rows, the 11-07 libm summary row and, per row present in both sweeps, the
change of the time ratio (new / old: same CPU class, different commit), verdict transitions, and the full table.
Percentiles use the sweep tool's rule (sorted[int(n * p)], capped at the last element). The 11-07 numbers are read
from the "Zen 3, libm rows" sections of results/timing-sweep-zen3-1107-table.md (that run is recorded in
handoff/timing-sweep-natsumi.md, which also records this run).
Produces results/timing-sweep-zen3-libm-1113-table.md.
"""
import re
import statistics
import sys

FASTER, SLOWER = 0.95, 1.05
# Counted rows the sweep tool reports outside its ratio_host selection (its summary line names them).
BASELINE = ("add_baseline", "mxcsr_pair", "mxcsr_wrapper_seq")


def num(s):
    return None if s in ("-", "") else float(s)


def verdict(t):
    return "faster" if t < FASTER else "slower" if t > SLOWER else "parity"


def f3(x):
    return "-" if x is None else f"{x:.3f}"


def pct(s, p):
    return s[min(int(len(s) * p), len(s) - 1)]


def load(path):
    rows = []
    with open(path) as f:
        head = f.readline().rstrip("\n").split("\t")
        for line in f:
            r = dict(zip(head, line.rstrip("\n").split("\t")))
            for k in ("instr", "cycles", "time", "spread"):
                r[k] = num(r[k])
            r["key"] = f"{r['bench']}/{r['row']}"
            rows.append(r)
    return rows


def load_old(path):
    """The 11-07 libm summary row and its all-rows table (bench/row -> (instr, time, verdict))."""
    with open(path) as f:
        text = f.read()
    sec = text.split("## Zen 3, libm rows\n", 1)[1]
    summ = re.search(r"^\| (\d+) \| (\d+) \| (\d+) \| (\d+) \| ([\d.]+) \| ([\d.]+) \| ([\d.]+) \| ([\d.]+) \| ([\d.]+) \|",
                     sec, re.M)
    if not summ:
        sys.exit(f"no libm summary row in {path}")
    allrows = text.split("### Zen 3, libm rows: all rows\n", 1)[1]
    old = {}
    for m in re.finditer(r"^\| (cbench_libm/\S+) \| (\S+) \| (\S+) \| (\S+) \| (\S+) \| (\S+) \| (\w+) \|$", allrows, re.M):
        old[m.group(1)] = (num(m.group(2)), num(m.group(4)), m.group(7))
    if not old:
        sys.exit(f"no libm rows in {path}")
    return summ.groups(), old


def main():
    if len(sys.argv) != 5:
        sys.exit(__doc__)
    rows = load(sys.argv[1])
    (o_n, o_f, o_p, o_s, o_med, _o_p10, _o_p90, o_geo, o_spr), old = load_old(sys.argv[2])
    new_c, old_c = sys.argv[3], sys.argv[4]
    judged = [r for r in rows if r["time"] is not None]
    for r in judged:
        # sweep.tsv prints 3 decimals and the tool judges the unrounded ratio: at a threshold its verdict stands.
        at_edge = min(abs(r["time"] - FASTER), abs(r["time"] - SLOWER)) <= 0.0005
        if verdict(r["time"]) != r["verdict"] and not at_edge:
            sys.exit(f"verdict mismatch {r['key']}: {r['time']} {r['verdict']}")
    c = {v: sum(1 for r in judged if r["verdict"] == v) for v in ("faster", "parity", "slower")}
    times = sorted(r["time"] for r in judged)
    spreads = sorted(r["spread"] for r in rows if r["spread"] is not None)
    instr = sorted(r["instr"] for r in judged if r["instr"] is not None)
    geo = statistics.geometric_mean([t for t in times if t > 0])
    out = ["## Zen 3, libm rows: summary", "",
           "| sweep | rows judged | faster | parity | slower | time median | time geomean | spread median | spread p90 | "
           "spread max | instr median (counted rows) |", "|---|---|---|---|---|---|---|---|---|---|---|"]
    out.append(f"| 11-13, next {new_c} | {len(judged)} | {c['faster']} | {c['parity']} | {c['slower']} | "
               f"{statistics.median(times):.3f} | {geo:.3f} | {statistics.median(spreads):.3f} | "
               f"{pct(spreads, 0.9):.3f} | {spreads[-1]:.3f} | {statistics.median(instr):.3f} ({len(instr)}) |")
    out.append(f"| 11-07, next {old_c} | {o_n} | {o_f} | {o_p} | {o_s} | {o_med} | {o_geo} | {o_spr} | - | - | - |")
    out.append("")
    # The sweep tool's own counts leave out the counted rows outside its ratio_host selection (baselines); the 11-07
    # table counted them. Both views, each sweep counted by the same rule.
    sel = [r for r in judged if r["row"] not in BASELINE]
    sc = {v: sum(1 for r in sel if r["verdict"] == v) for v in ("faster", "parity", "slower")}
    oc = {v: sum(1 for k, o in old.items() if k.split("/", 1)[1] not in BASELINE and o[2] == v)
          for v in ("faster", "parity", "slower")}
    out.append(f"Without the {len(BASELINE)} baseline rows outside the ratio_host selection ({', '.join(BASELINE)}): "
               f"11-13 {len(sel)} rows, {sc['faster']} / {sc['parity']} / {sc['slower']} (the sweep tool's own count); "
               f"11-07 {sum(oc.values())} rows, {oc['faster']} / {oc['parity']} / {oc['slower']}.")
    flagged = [r for r in rows if r.get("flagged") == "yes"]
    out.append(f"Rows flagged by the bench's per-host limit check: {len(flagged)}"
               + ("" if flagged else " (no limits exist for this CPU model)."))
    out += [f"- {r['key']} time {f3(r['time'])} limit {r['limit']} status {r['status']}" for r in flagged]
    noisy = [r for r in judged if r["spread"] is not None and r["spread"] > 0.10 and r["verdict"] != "parity"
             and abs(r["time"] - 1) < r["spread"]]
    out.append(f"Judged rows whose verdict lies inside their own spread (spread > 0.10 and |time - 1| < spread): "
               f"{len(noisy)}" + ("" if not noisy else ": " + ", ".join(r["key"] for r in noisy)))

    both = [r for r in judged if r["key"] in old and old[r["key"]][1]]
    only_new = sorted(r["key"] for r in judged if r["key"] not in old)
    only_old = sorted(k for k in old if k not in {r["key"] for r in rows})
    chg = sorted(r["time"] / old[r["key"]][1] for r in both)
    trans = {}
    for r in both:
        t = (old[r["key"]][2], r["verdict"])
        trans[t] = trans.get(t, 0) + 1
    out += ["", "## Change against the 11-07 Zen 3 libm sweep (same CPU class, different commit)", "",
            f"Rows in both sweeps: {len(both)}; only in 11-13: {len(only_new)}"
            + (" (" + ", ".join(only_new) + ")" if only_new else "")
            + f"; only in 11-07: {len(only_old)}" + (" (" + ", ".join(only_old) + ")" if only_old else "") + ".",
            f"Change = 11-13 time ratio / 11-07 time ratio (< 1: Liella moved closer to or further ahead of glibc). "
            f"Median {statistics.median(chg):.3f}, geomean {statistics.geometric_mean(chg):.3f}, "
            f"p10 {pct(chg, 0.1):.3f}, p90 {pct(chg, 0.9):.3f}; rows with change < 0.95: "
            f"{sum(1 for x in chg if x < 0.95)}, 0.95-1.05: {sum(1 for x in chg if 0.95 <= x <= 1.05)}, "
            f"> 1.05: {sum(1 for x in chg if x > 1.05)}.", "",
            "| 11-07 verdict -> 11-13 verdict | rows |", "|---|---|"]
    order = {"faster": 0, "parity": 1, "slower": 2}
    out += [f"| {a} -> {b} | {n} |" for (a, b), n in sorted(trans.items(), key=lambda kv: (order[kv[0][0]], order[kv[0][1]]))]

    def oldt(r):
        return f3(old[r["key"]][1]) if r["key"] in old else "-"

    def ch(r):
        return f3(r["time"] / old[r["key"]][1]) if r in both else "-"

    out += ["", "### Zen 3, libm rows: slowest 20 rows (11-13)", "",
            "| bench/row | time | instr | cycles | spread | rounds | time 11-07 | change |", "|---|---|---|---|---|---|---|---|"]
    for r in sorted(judged, key=lambda r: -r["time"])[:20]:
        out.append(f"| {r['key']} | {f3(r['time'])} | {f3(r['instr'])} | {f3(r['cycles'])} | {f3(r['spread'])} | "
                   f"{r['rounds']} | {oldt(r)} | {ch(r)} |")
    out += ["", "### Zen 3, libm rows: all rows", "",
            "| bench/row | instr | cycles | time | spread | rounds | verdict | time 11-07 | verdict 11-07 | change |",
            "|---|---|---|---|---|---|---|---|---|---|"]
    for r in sorted(rows, key=lambda r: r["key"]):
        ov = old[r["key"]][2] if r["key"] in old else "-"
        out.append(f"| {r['key']} | {f3(r['instr'])} | {f3(r['cycles'])} | {f3(r['time'])} | {f3(r['spread'])} | "
                   f"{r['rounds']} | {r['verdict']} | {oldt(r)} | {ov} | {ch(r)} |")
    print("\n".join(out))


main()
