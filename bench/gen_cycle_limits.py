#!/usr/bin/env python3
"""Cycle limits for a cycles CPU (FALLBACK-CYCLES) from a full calibration bench log.

    gen_cycle_limits.py CALIB.log "CPU MODEL" SHA HOST >> bench/host_cycle_limits.tsv

Same rule as the ns limits of bench/host_limits.tsv (work/hostlimits/gen.py; kanon fe1f24a, kotori 5236e77):
limit = 1.25 x the same-run value, 3 decimals, status gate. The value here is the CYCLES ratio of the
counters table ("counters per operation", median of 3 counted runs per side, user+kernel cycles) for each
row of the log's "ratio_host rows" section. A row whose cycles column is `-` (no counter) gets no cycle
limit and keeps its ns limit. The calibration log must come from a bench run with --counters (task benches
and `--counters` runs print the table; a log without it yields no lines and says so on stderr).
"""
import re
import sys


def parse(lines):
    host_rows, cyc = set(), {}
    sec = None
    for ln in lines:
        if ln.startswith("ratio_host rows"):
            sec = "rh"
            continue
        if ln.startswith("counters per operation"):
            sec = "ctr"
            continue
        if sec == "rh":
            m = re.match(r"\s+(\w+)/(\w+): median ", ln)
            if m:
                host_rows.add((m[1], m[2]))
            elif ln.strip() == "" or ln.startswith("bench:"):
                sec = None
        elif sec == "ctr":
            f = ln.split()
            if len(f) >= 4 and f[0].startswith("cbench_"):
                try:
                    cyc[(f[0], f[1])] = float(f[3])
                except ValueError:
                    pass  # `-`: no cycles counter for this row
            elif ln.startswith("bench:"):
                sec = None
    return host_rows, cyc


def main(argv):
    log, cpu, sha, host = argv[1:5]
    with open(log, encoding="utf-8", errors="replace") as f:
        host_rows, cyc = parse(f)
    # A ratio that prints as 0.000 gives no usable limit (kotori: regexec_pathological_4k, left out of the ns limits too).
    out = [(b, r, cyc[(b, r)]) for (b, r) in sorted(host_rows) if (b, r) in cyc and round(cyc[(b, r)] * 1.25, 3) > 0]
    if not out:
        print(f"gen_cycle_limits: no cycles ratios for ratio_host rows in {log} (run the calibration with --counters)", file=sys.stderr)
        return 1
    print(f"# {host} ({cpu}): cycle limits from the calibration bench of main {sha} ({log.split('/')[-1]}); rule 1.25 x same-run cycles ratio (FALLBACK-CYCLES).")
    for b, r, v in out:
        note = "; cycles ratio above 1.1" if v > 1.1 else ""
        print(f"{cpu}\t{b}\t{r}\t{round(v * 1.25, 3):g}\tgate\t1.25 x cycles ratio {v:.3f} on this CPU ({host} calibration, main {sha}{note})")
    print(f"gen_cycle_limits: {len(out)} of {len(host_rows)} ratio_host rows have a cycles ratio", file=sys.stderr)
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
