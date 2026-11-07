import sys, statistics, collections
# Lines: "pass=P round=R prog=X <row>\t<ns>". Prints per pass, per row: median ns per prog, spread, ratio to host-dynamic.
d = collections.defaultdict(list)
for line in open(sys.argv[1]):
    if not line.startswith("pass="):
        continue
    head, ns = line.rsplit("\t", 1)
    f = head.split()
    p, prog, row = f[0][5:], f[2][5:], f[3]
    d[(p, row, prog)].append(float(ns))
    d[("all", row, prog)].append(float(ns))
for key in sorted({(k[0], k[1]) for k in d}):
    ref = statistics.median(d[key + ("host-dynamic",)])
    out = []
    for prog in sorted({k[2] for k in d if k[:2] == key}):
        v = d[key + (prog,)]
        m = statistics.median(v)
        out.append(f"{prog} {m:.2f}ns sp{(max(v)-min(v))/m:.2f} x{m/ref:.2f}")
    print(f"pass {key[0]} {key[1]}: " + " | ".join(out))
