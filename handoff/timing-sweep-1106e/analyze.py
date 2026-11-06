# TIMING-SWEEP-1106E analyzer (from the TIMING-SWEEP-1106 one): python3 -I analyze.py <dir> <1106-table.md>
# <dir>/{ivb,n100}/{counters,rounds}.tsv -> <dir>/table.md, <dir>/lists.md
# Only the rows under "## Table" of the 1106 file (and its wordexp table) are read as the old values.
import csv, statistics as st, sys, collections
base, t1106 = sys.argv[1:3]
hosts = ("ivb", "n100")
NAME = {"ivb": "Ivy Bridge", "n100": "N100"}


def load(h):
    d = f"{base}/{h}/"
    cnt = {(r["bench"], r["row"]): r for r in csv.DictReader(open(d + "counters.tsv"), delimiter="\t")}
    rounds = collections.defaultdict(list)
    for r in csv.DictReader(open(d + "rounds.tsv"), delimiter="\t"):
        try:
            rounds[(r["bench"], r["row"])].append(float(r["liella_ns"]) / float(r["host_ns"]))
        except (ValueError, ZeroDivisionError):
            pass
    return cnt, rounds


def num(s):
    try:
        return float(s)
    except (TypeError, ValueError):
        return None


def mdrows(path):
    on = False
    for line in open(path):
        if line.startswith("## Table"):
            on = True
        c = [x.strip() for x in line.strip().strip("|").split("|")]
        if on and len(c) > 8 and c[0].startswith("cbench_"):
            yield c


old = {}  # key -> (instr ivb, time ivb, instr n100, time n100)
for c in mdrows(t1106):
    old[tuple(c[0].split("/", 1))] = (num(c[1]), num(c[2]), num(c[7]), num(c[8]))

data = {h: load(h) for h in hosts}
keys = set()
for h in hosts:
    keys |= set(data[h][0]) | set(data[h][1])
keys = sorted(k for k in keys if k[0] != "cbench_libm")


def verdict(x):
    if x is None:
        return "-"
    return "faster" if x < 0.95 else ("slower" if x > 1.05 else "parity")


def f(x):
    return "-" if x is None else f"{x:.3f}"


rows = []
for key in keys:
    rec = {"key": key, "wordexp": "wordexp" in key[1]}
    o = old.get(key)
    for i, h in enumerate(hosts):
        cnt, rounds = data[h]
        rr = rounds.get(key, [])
        c = cnt.get(key)
        t = st.median(rr) if rr else (num(c.get("ns_ratio")) if c else None)
        rec[h] = dict(
            t=t,
            sp=(max(rr) - min(rr)) / t if rr and t else None,
            cy=num(c.get("cycles_ratio")) if c else None,
            ins=num(c.get("instr_ratio")) if c else None,
            n=len(rr),
            old=o[1 + 2 * i] if o else None,
            oldins=o[2 * i] if o else None,
        )
    rows.append(rec)


def line(r):
    cells = [f"{r['key'][0]}/{r['key'][1]}"]
    for h in hosts:
        x = r[h]
        ch = f"{x['t'] / x['old']:.3f}" if x["t"] and x["old"] else "-"
        cells += [f(x["ins"]), f(x["t"]), f(x["sp"]), verdict(x["t"]), f(x["old"]), ch]
    return "| " + " | ".join(cells) + " |"


HDR = ("| bench/row | instr IVB | time IVB | spread IVB | verdict IVB | 1106 time IVB | now/1106 IVB "
       "| instr N100 | time N100 | spread N100 | verdict N100 | 1106 time N100 | now/1106 N100 |")
SEP = "|" + "---|" * 13
out = [HDR, SEP] + [line(r) for r in rows if not r["wordexp"]]
out += ["", "### OWNER-DEFERRED: wordexp rows (timed, not judged)", "", HDR, SEP] + [line(r) for r in rows if r["wordexp"]]
open(base + "/table.md", "w").write("\n".join(out) + "\n")

L = collections.defaultdict(list)
spreads = {h: [] for h in hosts}
drift = {h: [] for h in hosts}
for r in rows:
    k = f"{r['key'][0]}/{r['key'][1]}"
    for h in hosts:
        x = r[h]
        if x["sp"] is not None:
            spreads[h].append(x["sp"])
        if x["t"] is None or r["wordexp"]:
            continue
        if x["old"]:
            drift[h].append(x["t"] / x["old"])
        v, ov = verdict(x["t"]), verdict(x["old"])
        desc = f"{k} | {NAME[h]} | instr {f(x['ins'])} (1106 {f(x['oldins'])}) | time {f(x['t'])} (1106 {f(x['old'])}) | spread {f(x['sp'])} | cycles {f(x['cy'])}"
        if v == "slower" and ov in ("parity", "faster"):
            L["a"].append(desc)
        if x["ins"] is not None and x["ins"] <= 1.05 and x["t"] > 1.05:
            L["b"].append(desc)
        if ov == "slower" and v in ("parity", "faster"):
            L["c"].append(desc)
        if x["old"] is None:
            L["new"].append(desc)
with open(base + "/lists.md", "w") as fh:
    for tag, title in (("a", "REGRESSIONS (slower now, parity/faster in 1106)"), ("b", "REOPEN (instr <= 1.05, time > 1.05)"),
                       ("c", "IMPROVED (slower in 1106, parity/faster now)"), ("new", "rows not in the 1106 table")):
        fh.write(f"## {title}: {len(L[tag])}\n")
        for x in L[tag]:
            fh.write(f"- {x}\n")
    for h in hosts:
        s = sorted(spreads[h])
        if s:
            fh.write(f"spread {h}: n={len(s)} median {st.median(s):.3f} p90 {s[int(len(s) * 0.9)]:.3f} max {s[-1]:.3f}\n")
        d = sorted(drift[h])
        if d:
            fh.write(f"drift now/1106 {h}: n={len(d)} median {st.median(d):.3f} p10 {d[int(len(d) * 0.1)]:.3f} p90 {d[int(len(d) * 0.9)]:.3f}\n")
        v = [verdict(r[h]["t"]) for r in rows if r[h]["t"] is not None and not r["wordexp"]]
        ov = [verdict(r[h]["old"]) for r in rows if r[h]["old"] is not None and not r["wordexp"]]
        fh.write(f"{h} now: " + ", ".join(f"{n} {v.count(n)}" for n in ("faster", "parity", "slower")) + f" of {len(v)}; 1106: " + ", ".join(f"{n} {ov.count(n)}" for n in ("faster", "parity", "slower")) + f" of {len(ov)}\n")
print(len(rows), {k: len(v) for k, v in L.items()}, "old rows", len(old))
