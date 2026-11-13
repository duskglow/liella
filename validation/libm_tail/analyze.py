#!/usr/bin/env python3
"""LIBM-TAIL-PROBE analysis (LAB-ONLY; no spec clause): turns validation/libm_tail/run.sh output into tables.

    python3 validation/libm_tail/analyze.py <outdir> <prefix>

Writes <prefix>-summary.tsv (per function and library: inputs, mean, median, p99, p99.99, max, share of inputs
over 2000 instructions), <prefix>-bands.tsv (runs of adjacent blocks where one library costs over 10x the other
or has inputs over 2000 instructions; ranked by fraction of inputs x extra cost) and <prefix>-binades.tsv (per
function and input binade: mean and max for each library). Costs are instructions per call with the harness
overhead removed; percentiles above 255 are histogram bucket lower bounds (16 buckets per octave, <= 6.25% low).
"""
import collections
import glob
import os
import struct
import sys

NB = 768


def bucket_value(b):
    if b < 256:
        return b
    e = 8 + (b - 256) // 16
    sub = (b - 256) % 16
    return (16 + sub) << (e - 4)


def load(outdir, tag):
    blocks = {}
    hist = collections.defaultdict(lambda: collections.Counter())
    meta = {}
    for path in glob.glob(os.path.join(outdir, tag + ".blocks.*")):
        with open(path) as f:
            for line in f:
                k, n, s, mx, arg, arg2, over = line.split()
                blocks[int(k)] = (int(n), int(s), int(mx), (int(arg, 16), int(arg2, 16)), int(over))
    for path in glob.glob(os.path.join(outdir, tag + ".hist.*")):
        with open(path) as f:
            for line in f:
                b, k, c = map(int, line.split())
                hist[b][k] += c
    for path in glob.glob(os.path.join(outdir, tag + ".meta.*")):
        with open(path) as f:
            for line in f:
                k, v = line.split(None, 1)
                meta.setdefault(k, []).append(v.strip())
    return blocks, hist, meta


def pct(counter, total, q):
    target = q * total
    acc = 0
    for k in sorted(counter):
        acc += counter[k]
        if acc >= target:
            return bucket_value(k)
    return 0


def f32(bits):
    return struct.unpack("<f", struct.pack("<I", bits & 0xFFFFFFFF))[0]


def f64(bits):
    return struct.unpack("<d", struct.pack("<Q", bits & 0xFFFFFFFFFFFFFFFF))[0]


def key_range(mode, key):
    """Human range of the inputs in block `key`."""
    if mode in ("u32", "x32"):
        return "[%.9g, %.9g]" % (f32(key << 16), f32((key << 16) | 0xFFFF))
    if mode in ("b32", "B32"):
        xb, yb = key >> 9, key & 511
        return "x[%.3g,%.3g) y[%.3g,%.3g)" % (f32(xb << 23), f32(((xb << 23) | 0x7FFFFF)), f32(yb << 23), f32((yb << 23) | 0x7FFFFF))
    if mode in ("u64", "U64"):
        return "[%.17g, %.17g]" % (f64(key << 46), f64((key << 46) | ((1 << 46) - 1)))
    if mode in ("b64", "B64"):
        xb, yb = key >> 9, key & 511
        return "x[%.3g,%.3g] y[%.3g,%.3g]" % (f64(xb << 55), f64((xb << 55) | ((1 << 55) - 1)), f64(yb << 55), f64((yb << 55) | ((1 << 55) - 1)))
    return "point#%d" % key


def arg_text(mode, arg):
    x, y = arg
    if mode in ("u32", "x32"):
        return "%.9g" % f32(x)
    if mode in ("b32", "B32"):
        return "(%.9g,%.9g)" % (f32(x), f32(y))
    if mode in ("b64", "B64"):
        return "(%.17g,%.17g)" % (f64(x), f64(y))
    return "%.17g" % f64(x)


def hot_ranges(outdir, tag, mode):
    """Runs of adjacent hot batches (mean over 2000) of an x32 run: (lo_bits, hi_bits, inputs, mean)."""
    rows = []
    for path in glob.glob(os.path.join(outdir, tag + ".hot.*")):
        with open(path) as f:
            for line in f:
                a, k, c = line.split()
                rows.append((int(a, 16), int(k), int(c)))
    rows.sort()
    out = []
    for a, k, c in rows:
        if out and mode == "x32" and out[-1][1] + 1 == a:
            lo, hi, n, s = out[-1]
            out[-1] = (lo, a + k - 1, n + k, s + c)
        else:
            out.append((a, a + k - 1, k, c))
    return out


def bands(mode, A, Bo, total, a_name):
    """Runs of adjacent keys where library A is over 10x B (block mean) or has inputs over 2000."""
    out = []
    run = []

    def close():
        if not run:
            return
        n = sum(A[k][0] for k in run)
        sa = sum(A[k][1] for k in run)
        sb = sum(Bo[k][1] for k in run if k in Bo)
        nb = sum(Bo[k][0] for k in run if k in Bo)
        over = sum(A[k][4] for k in run)
        kmax = max(run, key=lambda k: A[k][2])
        out.append({
            "slow": a_name, "lo": run[0], "hi": run[-1], "blocks": len(run), "n": n,
            "frac": n / total, "over_frac": over / total,
            "mean_slow": sa / n, "mean_other": (sb / nb) if nb else float("nan"),
            "max_slow": A[kmax][2], "argmax": arg_text(mode, A[kmax][3]),
            "score": (sa / n - ((sb / nb) if nb else 0)) * n / total,
            "range": key_range(mode, run[0]) + (" .. " + key_range(mode, run[-1]) if len(run) > 1 else ""),
        })
        run.clear()

    prev = None
    for k in sorted(A):
        n, s, mx, arg, over = A[k]
        ma = s / n
        if k in Bo:
            mb = Bo[k][1] / Bo[k][0]
        else:
            mb = None
        flag = over > 0 or (mb is not None and ma > 10 * max(mb, 1.0))
        if flag and prev is not None and k == prev + 1 and run:
            run.append(k)
        elif flag:
            close()
            run.append(k)
        else:
            close()
        prev = k
    close()
    return out


def main():
    outdir, prefix = sys.argv[1], sys.argv[2]
    tags = sorted({os.path.basename(p).rsplit(".", 1)[0][: -len(".glibc")]
                   for p in glob.glob(os.path.join(outdir, "*.glibc.done"))})
    summ, bandrows, binrows, hotrows = [], [], [], []
    for tag in tags:
        func, mode, S = tag.split(".")
        if not os.path.exists(os.path.join(outdir, tag + ".liella.done")):
            continue
        data = {lib: load(outdir, tag + "." + lib) for lib in ("glibc", "liella")}
        for lib in ("glibc", "liella"):
            blocks, hist, meta = data[lib]
            allc = collections.Counter()
            for b in hist.values():
                allc.update(b)
            total = sum(allc.values())
            ssum = sum(v[1] for v in blocks.values())
            over = sum(v[4] for v in blocks.values())
            kmax = max(blocks, key=lambda k: blocks[k][2])
            summ.append([func, mode, S, lib, total, "%.1f" % (ssum / total), pct(allc, total, 0.5), pct(allc, total, 0.99),
                         pct(allc, total, 0.9999), blocks[kmax][2], arg_text(mode, blocks[kmax][3]), "%.3g" % (over / total),
                         ",".join(sorted(set(meta.get("overhead", [])))), ",".join(sorted(set(meta.get("batch", ["1"]))))])
        (gb, gh, _), (lb, lh, _) = data["glibc"], data["liella"]
        total = sum(v[0] for v in lb.values())
        for a_name, A, Bo in (("liella", lb, gb), ("glibc", gb, lb)):
            for r in bands(mode, A, Bo, total, a_name):
                bandrows.append([func, mode, S] + [r[c] for c in ("slow", "range", "blocks", "n", "frac", "over_frac", "mean_slow",
                                                                  "mean_other", "max_slow", "argmax", "score")])
        if mode == "x32":
            for lib in ("liella", "glibc"):
                for lo, hi, n, s in hot_ranges(outdir, tag + "." + lib, mode):
                    hotrows.append([func, lib, "%.9g" % f32(lo), "%.9g" % f32(hi), "%08x" % lo, "%08x" % hi, n,
                                    "%.3g" % (n / total), "%.0f" % (s / n), "%.4g" % (s / total)])
        # per-binade profile (binade = histogram row: input sign+exponent; x for binary modes)
        for b in sorted(set(gh) | set(lh)):
            row = [func, mode, S, b]
            for h in (gh, lh):
                c = h.get(b, collections.Counter())
                n = sum(c.values())
                if n:
                    mean = sum(bucket_value(k) * v for k, v in c.items()) / n
                    row += [n, "%.0f" % mean, pct(c, n, 0.99), bucket_value(max(c))]
                else:
                    row += [0, "", "", ""]
            binrows.append(row)
    with open(prefix + "-summary.tsv", "w") as f:
        f.write("func\tmode\tS\tlib\tinputs\tmean\tmedian\tp99\tp99.99\tmax\targmax\tfrac_over2000\toverhead\tunit_inputs\n")
        for r in summ:
            f.write("\t".join(map(str, r)) + "\n")
    bandrows.sort(key=lambda r: -r[-1])
    with open(prefix + "-bands.tsv", "w") as f:
        f.write("func\tmode\tS\tslow_lib\trange\tblocks\tinputs\tfrac\tfrac_over2000\tmean_slow\tmean_other\tmax_slow\targmax\tscore\n")
        for r in bandrows:
            f.write("\t".join(("%.4g" % v) if isinstance(v, float) else str(v) for v in r) + "\n")
    with open(prefix + "-hot.tsv", "w") as f:
        f.write("func\tlib\tfrom\tto\tfrom_bits\tto_bits\tinputs\tfrac\tmean\tcost_share_per_input\n")
        for r in sorted(hotrows, key=lambda r: -float(r[-1])):
            f.write("\t".join(map(str, r)) + "\n")
    with open(prefix + "-binades.tsv", "w") as f:
        f.write("func\tmode\tS\tbinade\tg_n\tg_mean\tg_p99\tg_max\tl_n\tl_mean\tl_p99\tl_max\n")
        for r in binrows:
            f.write("\t".join(map(str, r)) + "\n")


if __name__ == "__main__":
    main()
