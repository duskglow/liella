import sys, statistics as st, collections as co
d = sys.argv[1]
def load(f):
    r = co.defaultdict(list)
    for ln in open(f"{d}/{f}"):
        p = ln.rstrip("\n").split("\t")
        if p[1] == "base-gross":
            print("BASE", f, p[0], p[2], p[4:]); continue
        v = [float(x) if x != "-" else None for x in p[4:8]]
        r[(p[0], p[1], p[3])].append(v)
    return r
L, G, N = load("count-liella.tsv"), load("count-glibc.tsv"), load("count-liella-noproto.tsv")
R = ["ascii", "latin1", "cjk", "bmpmix", "astral", "bmprnd"]
def m(rows, i):
    xs = [x[i] for x in rows if x[i] is not None]
    return st.median(xs) if xs else float("nan")
def rng(rows, i):
    xs = sorted(x[i] for x in rows if x[i] is not None)
    return f"{xs[0]:.0f}-{xs[-1]:.0f}" if xs else "-"
mode = sys.argv[2] if len(sys.argv) > 2 else "lib"
for loc in ["C", "C.UTF-8", "en_US.UTF-8"]:
    print("==", loc, "| per range: ins L/G (class min-max L) cyc L/G | Liella taken, loads")
    ents = ["isw", "isw_l", "iswctype", "iswctype_l", "towupper", "towupper_l", "towctrans"] if mode == "lib" else \
        ["pA_isw", "pA_isw_l", "pA_iswctype", "pA_iswctype_l", "pB_isw", "pB_isw_l", "pB_iswctype", "pB_iswctype_l"]
    for e in ents:
        cells = []
        for rg in R:
            l = L.get((loc, e, rg), [])
            if not l: continue
            if mode == "lib":
                g = G[(loc, e, rg)]
                cells.append(f"{m(l,0):.0f}/{m(g,0):.0f}({rng(l,0)}) {m(l,1):.0f}/{m(g,1):.0f} t{m(l,2):.1f} ld{m(l,3):.1f}")
            else:
                cells.append(f"{m(l,0):.0f} c{m(l,1):.1f} t{m(l,2):.1f} ld{m(l,3):.1f}")
        print(f"{e:13s}", " | ".join(cells))
if mode == "lib":
    diffs = [abs(m(L[k], 0) - m(N[k], 0)) for k in N if k in L]
    print("proto-vs-noproto Liella ins |diff| median %.2f max %.2f" % (st.median(diffs), max(diffs)))
