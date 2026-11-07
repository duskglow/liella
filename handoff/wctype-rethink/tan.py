import sys, glob, statistics as st, collections as co
K = {"iswalpha": "f(c)", "iswalpha_l": "f(c,l)", "iswctype": "f(c,t)", "iswctype_l": "f(c,t,l)", "towupper": "g(c)", "towupper_l": "g(c,l)"}
R = ["ascii", "latin1", "cjk", "bmpmix", "astral", "bmprnd"]
for host in sys.argv[1:]:
    net = co.defaultdict(list)  # (side, loc, entry, range) -> per-round net ns
    gross = co.defaultdict(list)
    for f in glob.glob(f"{host}/t-*.tsv"):
        side = "L" if "liella" in f else "G"
        rows = {}
        for ln in open(f):
            rd, loc, name, rg, ns = ln.split("\t")
            rows[(rd, loc, name, rg)] = float(ns)
        for (rd, loc, name, rg), ns in rows.items():
            if name in K or name[3:] in K:
                base = K.get(name) or K[name[3:]]
                net[(side, loc, name, rg)].append(ns - rows[(rd, loc, base, rg)])
                gross[(side, loc, name, rg)].append(ns)
    print("##", host, "net ns/call median (nop-subtracted); cells: Liella / glibc / pA / pB ; ratios L/G pA/G")
    for loc in ["C.UTF-8", "en_US.UTF-8"]:
        print("==", loc)
        for e in ["iswalpha", "iswalpha_l", "iswctype", "iswctype_l", "towupper", "towupper_l"]:
            cells = []
            for rg in R:
                l = st.median(net[("L", loc, e, rg)]); g = st.median(net[("G", loc, e, rg)])
                pa = net.get(("L", loc, "pA_" + e, rg)); pb = net.get(("L", loc, "pB_" + e, rg))
                s = f"{l:.2f}/{g:.2f}"
                if pa:
                    pa, pb = st.median(pa), st.median(pb)
                    s += f"/{pa:.2f}/{pb:.2f} {l/g:.2f} {pa/g:.2f}"
                else:
                    s += f" {l/g:.2f}"
                cells.append(s)
            print(f"{e:11s}", " | ".join(cells))
    nb = [st.median(gross[k]) for k in gross if k[2] in ("iswalpha",)]
