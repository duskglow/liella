import csv, statistics as st, sys, collections
base = sys.argv[1]
hosts = [("ivb", "nico"), ("n100", "nozomi")]

def load(h):
    d = f"{base}/{h}/"
    cnt = {}
    for r in csv.DictReader(open(d + "counters.tsv"), delimiter="\t"):
        cnt[(r["bench"], r["row"])] = r
    rounds = collections.defaultdict(list)
    for r in csv.DictReader(open(d + "rounds.tsv"), delimiter="\t"):
        try:
            rounds[(r["bench"], r["row"])].append(float(r["liella_ns"]) / float(r["host_ns"]))
        except Exception:
            pass
    return cnt, rounds

data = {k: load(h) for k, h in hosts}
keys = set()
for k in data:
    keys |= set(data[k][0]) | set(data[k][1])
keys = sorted(k for k in keys if k[0] != "cbench_libm")

def verdict(x):
    if x is None:
        return "-"
    return "faster" if x < 0.95 else ("slower" if x > 1.05 else "parity")

def f(x):
    return "-" if x is None else f"{x:.3f}"

rows = []
for key in keys:
    rec = {"key": key}
    for k, _ in hosts:
        cnt, rounds = data[k]
        rr = rounds.get(key, [])
        c = cnt.get(key)
        t = st.median(rr) if rr else (float(c["ns_ratio"]) if c and c["ns_ratio"] not in ("", "-") else None)
        sp = (max(rr) - min(rr)) / t if rr and t else None
        cy = float(c["cycles_ratio"]) if c and c["cycles_ratio"] not in ("", "-") else None
        ins = float(c["instr_ratio"]) if c and c["instr_ratio"] not in ("", "-") else None
        rec[k] = dict(t=t, sp=sp, cy=cy, ins=ins, n=len(rr))
    rows.append(rec)

out = []
out.append("| bench/row | instr (IVB) | time IVB | spread IVB | verdict IVB | instr (N100) | time N100 | spread N100 | verdict N100 |")
out.append("|---|---|---|---|---|---|---|---|---|")
dis_reopen, dis_close = [], []
spreads = {k: [] for k, _ in hosts}
for r in rows:
    a, b = r["ivb"], r["n100"]
    for k in ("ivb", "n100"):
        if r[k]["sp"] is not None:
            spreads[k].append(r[k]["sp"])
    out.append(f"| {r['key'][0]}/{r['key'][1]} | {f(a['ins'])} | {f(a['t'])} | {f(a['sp'])} | {verdict(a['t'])} | {f(b['ins'])} | {f(b['t'])} | {f(b['sp'])} | {verdict(b['t'])} |")
    for k, name in (("ivb", "Ivy Bridge"), ("n100", "N100")):
        x = r[k]
        if x["ins"] is None or x["t"] is None:
            continue
        if x["ins"] <= 1.05 and x["t"] > 1.05:
            dis_reopen.append((r["key"], name, x["ins"], x["t"], x["cy"]))
        if x["ins"] > 1.05 and x["t"] <= 1.05:
            dis_close.append((r["key"], name, x["ins"], x["t"], x["cy"]))
open(base + "/table.md", "w").write("\n".join(out) + "\n")
with open(base + "/dis.md", "w") as fh:
    fh.write("REOPEN (instr <= 1.05, time > 1.05)\n")
    for x in dis_reopen:
        fh.write(f"{x[0][0]}/{x[0][1]} | {x[1]} | instr {x[2]:.3f} | time {x[3]:.3f} | cycles {f(x[4])}\n")
    fh.write("CLOSE (instr > 1.05, time <= 1.05)\n")
    for x in dis_close:
        fh.write(f"{x[0][0]}/{x[0][1]} | {x[1]} | instr {x[2]:.3f} | time {x[3]:.3f} | cycles {f(x[4])}\n")
    for k in spreads:
        s = spreads[k]
        if s:
            fh.write(f"spread {k}: n={len(s)} median {st.median(s):.3f} p90 {sorted(s)[int(len(s)*0.9)]:.3f} max {max(s):.3f}\n")
    for k, _ in hosts:
        v = [verdict(r[k]["t"]) for r in rows if r[k]["t"] is not None]
        fh.write(f"{k}: " + ", ".join(f"{n} {v.count(n)}" for n in ("faster", "parity", "slower")) + f" of {len(v)}\n")
print(len(rows), len(dis_reopen), len(dis_close))
