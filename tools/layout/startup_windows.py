#!/usr/bin/env python3
# PROTOTYPE (LAYOUT-CONTRACTS-PLAN; design in handoff/layout-contracts-plan.md): layout contract
# STARTUP-TEXT-WINDOWS, checked on built products, no timing and no tracing at check time.
#
# A static program's text is a file mapping; the first touch of each 64 KiB window of it is one
# page fault (fault-around maps the window). The contract: the functions the startup-only probe
# executes (keys in tools/layout/startup-text.keys, first-touch order, from the hot-text tracer)
# occupy at most <budget> distinct 64 KiB windows of the probe linked from the products, and none
# outside the hot group straddles a 64 KiB boundary. A failure names the contract and every
# function that spans a window the budget has no room for.
#
# usage: startup_windows.py <products dir> [--budget N] [--keys FILE] [--root DIR]
#   <products dir> holds libc.a and liella-crt1.o (target/liella after `cargo xtask so`).
# exit 0 = holds, 1 = broken (names the contract and the symbols), 2 = cannot check.
import os
import re
import subprocess
import sys
import tempfile

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, os.path.join(HERE, "..", "hottext"))
import keys  # noqa: E402  (build-stable keys: crate hashes stripped, as in hot-text.list)

WINDOW = 1 << 16
CONTRACT = "STARTUP-TEXT-WINDOWS"


def die(msg):
    print(f"layout: {CONTRACT}: cannot check: {msg}", file=sys.stderr)
    sys.exit(2)


def args():
    a = sys.argv[1:]
    if not a or a[0].startswith("-"):
        die("usage: startup_windows.py <products dir> [--budget N] [--keys FILE] [--root DIR]")
    opts = {"products": a[0], "budget": None, "keys": os.path.join(HERE, "startup-text.keys"),
            "root": os.path.normpath(os.path.join(HERE, "..", ".."))}
    i = 1
    while i < len(a):
        if a[i] in ("--budget", "--keys", "--root") and i + 1 < len(a):
            opts[a[i][2:]] = a[i + 1]
            i += 2
        else:
            die(f"unknown argument {a[i]}")
    return opts


def read_keys(path):
    budget, out = None, []
    for line in open(path):
        line = line.strip()
        m = re.match(r"#\s*budget\s+(\d+)", line)
        if m:
            budget = int(m.group(1))
        elif line and not line.startswith("#"):
            out.append(line)
    return budget, out


def link(products, root, work):
    exe, mapf = os.path.join(work, "startup_probe"), os.path.join(work, "startup_probe.map")
    cmd = ["cc", "-std=gnu17", "-O1", "-static", "-nostdlib", "-fno-builtin",
           os.path.join(products, "liella-crt1.o"), os.path.join(root, "crates/core/tests/oracle/page_faults.c"),
           os.path.join(products, "libc.a"), "-lgcc", "-Wl,-Map," + mapf, "-o", exe]
    r = subprocess.run(cmd, capture_output=True, text=True)
    if r.returncode:
        die("the startup probe does not link: " + r.stderr.strip()[-400:])
    return exe, mapf


def functions(exe):
    """key -> list of (start, end, linked name) for every defined function of the linked probe."""
    r = subprocess.run(["nm", "-S", "--defined-only", exe], capture_output=True, text=True)
    out = {}
    for line in r.stdout.splitlines():
        p = line.split()
        if len(p) == 4 and p[2] in "tTwWiI":
            start, size = int(p[0], 16), int(p[1], 16)
            if size:
                out.setdefault(keys.key(p[3]), []).append((start, start + size, p[3]))
    return out


def hot_range(mapf):
    """[first, last) of the grouped hot text (`.text.sorted.h*` / `.text.hot.*` input sections)."""
    lo, hi, pend = None, None, False
    for line in open(mapf):
        if re.match(r"^ \.text\.(sorted\.h\d+|hot)\.", line):
            pend = True
            m = re.match(r"^ \S+\s+0x([0-9a-f]+)\s+0x([0-9a-f]+)", line)
        elif pend:
            m = re.match(r"^\s+0x([0-9a-f]+)\s+0x([0-9a-f]+)", line)
        else:
            continue
        if m:
            pend = False
            s, n = int(m.group(1), 16), int(m.group(2), 16)
            if n:
                lo = s if lo is None else min(lo, s)
                hi = s + n if hi is None else max(hi, s + n)
    if lo is None:
        die("no grouped hot text in the link map (D-X.20 grouping missing?)")
    return lo, hi


def main():
    o = args()
    budget, want = read_keys(o["keys"])
    if o["budget"] is not None:
        budget = int(o["budget"])
    if budget is None or not want:
        die(f"{o['keys']}: no '# budget N' line or no keys")
    with tempfile.TemporaryDirectory(prefix="layout-") as work:
        exe, mapf = link(o["products"], o["root"], work)
        fns = functions(exe)
        lo, hi = hot_range(mapf)
    windows, missing, owners = set(), [], {}
    for k in want:
        if k not in fns:
            missing.append(k)
            continue
        for s, e, name in fns[k]:
            for w in range(s // WINDOW, (e - 1) // WINDOW + 1):
                windows.add(w)
                owners.setdefault(w, []).append((s, e, name))
    group = set(range(lo // WINDOW, (hi - 1) // WINDOW + 1))
    outside = sorted(windows - group)
    straddle = sorted({(s, e, n) for w in outside for (s, e, n) in owners[w]
                       if not lo <= s < hi and s // WINDOW != (e - 1) // WINDOW})
    print(f"layout: {CONTRACT}: {len(windows)} windows (budget {budget}); hot group 0x{lo:x}-0x{hi:x} "
          f"({len(group & windows)} used); outside it: " + (", ".join(f"0x{w:x}" for w in outside) or "none")
          + f"; {len(want) - len(missing)} of {len(want)} startup functions found")
    for k in missing:
        print(f"  not in this link (inlined or renamed): {k}")
    bad = []
    if len(windows) > budget:
        bad.append(f"{len(windows)} startup text windows > budget {budget}")
    for s, e, n in straddle:
        bad.append(f"{n} (0x{s:x}-0x{e:x}, outside the hot group) straddles the 64 KiB boundary 0x{(e - 1) // WINDOW * WINDOW:x}")
    if bad:
        for w in outside:
            for s, e, n in owners[w]:
                print(f"  window 0x{w:x}0000: {n} 0x{s:x}-0x{e:x}")
        print(f"layout: {CONTRACT} BROKEN:\n  " + "\n  ".join(bad), file=sys.stderr)
        sys.exit(1)
    print(f"layout: {CONTRACT}: holds")


if __name__ == "__main__":
    main()
