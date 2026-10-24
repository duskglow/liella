#!/usr/bin/env python3
"""CONSISTENT-DRAG detector for the cycles-decided timing hosts (FALLBACK-CYCLES; owner 2026-10-10: "keep an eye
on time too, if we see a consistent drag, it needs to be investigated"). It FLAGS; it never fails a gate.

    timing_history.py append HISTORY.tsv DECISION.tsv --host H --branch B --commit C --label L
    timing_history.py summary HISTORY.tsv [--last 5] [--thr 1.05] [--min 3] [--ghz 0.05]
    timing_history.py selftest

DECISION.tsv = target/liella/bench/decision.tsv of one `cargo xtask bench` run on a cycles CPU (columns bench row
metric ns_ratio cycles_ratio ghz_l ghz_g limit verdict). `append` adds its rows to HISTORY.tsv (created with a header).
`summary` looks at each row's LAST N runs across all hosts in the history (both fallback hosts) and lists:
  DRAG  rows whose ns (time) ratio exceeded THR in >= MIN of those runs while the row passed on cycles;
  GHZ   rows where one side ran at a systematically different clock: |GHz L / GHz G - 1| > GHZ in >= MIN of the
        runs, all in the same direction (a library that keeps the core in a lower P-state is a real drag that
        cycles hide).
Exit status is always 0 (findings are printed, never a gate verdict).
"""
import csv
import io
import os
import sys
import tempfile
import time

COLS = ["date", "host", "branch", "commit", "label", "bench", "row", "metric", "ns_ratio", "cycles_ratio", "ghz_l", "ghz_g", "verdict"]


def num(s):
    try:
        return float(s)
    except (TypeError, ValueError):
        return None


def append(hist, decision, host, branch, commit, label, date=None):
    with open(decision, encoding="utf-8", newline="") as f:
        rows = list(csv.DictReader(f, delimiter="\t"))
    new = not os.path.exists(hist) or os.path.getsize(hist) == 0
    date = date or time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime())
    with open(hist, "a", encoding="utf-8", newline="\n") as f:
        if new:
            f.write("\t".join(COLS) + "\n")
        for r in rows:
            vals = [date, host, branch, commit, label] + [r.get(k, "-") for k in COLS[5:]]
            f.write("\t".join(v.replace("\t", " ") for v in vals) + "\n")
    return len(rows)


def summary(hist, last=5, thr=1.05, mn=3, ghz=0.05):
    with open(hist, encoding="utf-8", newline="") as f:
        rows = list(csv.DictReader(f, delimiter="\t"))
    per = {}
    for r in rows:
        per.setdefault((r["bench"], r["row"]), []).append(r)
    drag, skew = [], []
    for key, runs in sorted(per.items()):
        runs = runs[-last:]
        hosts = sorted({r["host"] for r in runs})
        d = [r for r in runs if (num(r["ns_ratio"]) or 0) > thr and r["metric"] == "cycles" and r["verdict"] == "ok"]
        if len(d) >= mn:
            ns = ", ".join(f"{num(r['ns_ratio']):.2f}" for r in runs)
            cy = ", ".join(r["cycles_ratio"] for r in runs)
            drag.append(f"DRAG {key[0]}/{key[1]}: time ratio > {thr} in {len(d)} of the last {len(runs)} runs while cycles passed (ns {ns}; cycles {cy}; hosts {' '.join(hosts)})")
        q = [num(r["ghz_l"]) / num(r["ghz_g"]) for r in runs if num(r["ghz_l"]) and num(r["ghz_g"])]
        lo, hi = sum(x < 1 - ghz for x in q), sum(x > 1 + ghz for x in q)
        if max(lo, hi) >= mn and min(lo, hi) == 0:
            side = "Liella slower clock" if lo else "Liella faster clock"
            skew.append(f"GHZ {key[0]}/{key[1]}: {side} in {max(lo, hi)} of the last {len(q)} runs (L/G {', '.join(f'{x:.2f}' for x in q)}; hosts {' '.join(hosts)})")
    out = [f"timing history: {len(rows)} entries, {len(per)} rows; consistent drag = time ratio > {thr} in >= {mn} of a row's last {last} runs (both fallback hosts) while cycles passed (flag only, never a gate)"]
    out += drag + skew
    out.append(f"timing history: {len(drag)} DRAG, {len(skew)} GHZ finding(s)" + ("" if drag or skew else " - none"))
    return out


def selftest():
    hdr = "bench\trow\tmetric\tns_ratio\tcycles_ratio\tghz_l\tghz_g\tlimit\tverdict\n"
    with tempfile.TemporaryDirectory() as d:
        hist = os.path.join(d, "h.tsv")
        # Run 1-4 on two hosts: `drag` passes cycles with time > 1.05 three times; `fine` never drags; `clock` keeps
        # Liella at a lower GHz every time; `failed` has slow time but FAILED on cycles (a gate failure, not a drag).
        canned = [
            ("nico", "drag\tcycles\t1.30\t1.01\t1.6\t2.4\t1.2\tok", "clock\tcycles\t1.00\t1.00\t1.80\t2.10\t1.2\tok"),
            ("kotori", "drag\tcycles\t1.10\t1.00\t2.0\t2.0\t1.2\tok", "clock\tcycles\t1.00\t1.00\t1.70\t2.00\t1.2\tok"),
            ("nico", "drag\tcycles\t1.02\t1.00\t2.0\t2.0\t1.2\tok", "clock\tcycles\t1.00\t1.00\t1.60\t2.20\t1.2\tok"),
            ("kotori", "drag\tcycles\t1.20\t1.02\t1.7\t2.3\t1.2\tok", "failed\tcycles\t1.40\t1.30\t2.0\t2.0\t1.2\tFAIL"),
        ]
        for i, (host, a, b) in enumerate(canned):
            p = os.path.join(d, f"dec{i}.tsv")
            with open(p, "w", encoding="utf-8", newline="\n") as f:
                f.write(hdr + f"cbench_x\t{a}\n" + f"cbench_x\t{b}\n" + "cbench_x\tfine\tcycles\t1.00\t1.00\t2.0\t2.0\t1.2\tok\n")
            append(hist, p, host, "task/t", f"c{i}", f"l{i}", date=f"d{i}")
        for _ in range(2):  # `failed` twice more: three slow runs, all failing on cycles -> no DRAG
            p = os.path.join(d, "decf.tsv")
            with open(p, "w", encoding="utf-8", newline="\n") as f:
                f.write(hdr + "cbench_x\tfailed\tcycles\t1.40\t1.30\t2.0\t2.0\t1.2\tFAIL\n")
            append(hist, p, "nico", "task/t", "cf", "lf", date="df")
        # A missing counter: metric ns, cycles `-`: never counted as "passed on cycles".
        p = os.path.join(d, "decm.tsv")
        with open(p, "w", encoding="utf-8", newline="\n") as f:
            f.write(hdr + "cbench_x\tnocyc\tns (cycles counter missing)\t1.30\t-\t-\t-\t1.2\tFAIL\n")
        for _ in range(3):
            append(hist, p, "nico", "task/t", "cm", "lm", date="dm")
        out = summary(hist)
        text = "\n".join(out)
        assert any(x.startswith("DRAG cbench_x/drag: time ratio > 1.05 in 3 of the last 4 runs") for x in out), text
        assert any(x.startswith("GHZ cbench_x/clock: Liella slower clock in 3 of the last 3 runs") for x in out), text
        assert not any("/fine" in x or "/failed" in x or "/nocyc" in x for x in out), text
        assert out[-1] == "timing history: 1 DRAG, 1 GHZ finding(s)", text
        assert summary(hist, mn=5)[-1].endswith(" - none"), text
        with open(hist, encoding="utf-8") as f:
            assert next(csv.DictReader(io.StringIO(f.read()), delimiter="\t"))["commit"] == "c0"
    print("timing_history selftest: ok")


def main(argv):
    if len(argv) < 2:
        print(__doc__)
        return 2
    cmd = argv[1]
    opts, pos, i = {}, [], 2
    while i < len(argv):
        if argv[i].startswith("--"):
            opts[argv[i][2:]] = argv[i + 1]
            i += 2
        else:
            pos.append(argv[i])
            i += 1
    if cmd == "selftest":
        selftest()
    elif cmd == "append":
        n = append(pos[0], pos[1], opts.get("host", "?"), opts.get("branch", "?"), opts.get("commit", "?"), opts.get("label", "?"))
        print(f"timing history: {n} rows appended to {pos[0]}")
    elif cmd == "summary":
        for line in summary(pos[0], int(opts.get("last", 5)), float(opts.get("thr", 1.05)), int(opts.get("min", 3)), float(opts.get("ghz", 0.05))):
            print(line)
    else:
        print(__doc__)
        return 2
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
