#!/usr/bin/env python3
"""Classify the page faults of one run of a static program by section and symbol.

Usage: faultmap.py <program> <perf-script-output>

<perf-script-output> is `perf script -F addr,ip,sym` of
`setarch -R perf record -e page-faults -c 1 -d <program> ...`. Prints one line per fault
(page, section, nearest symbol at or below the address, the faulting ip's symbol) and a count
per section. Addresses outside the program's sections (heap, stack, mappings, vdso) are
`other`. Used to see which page each start-up fault touches (STARTUP-FAULTS-RESTORE).
"""
import bisect
import subprocess
import sys


def sections(prog):
    out = []
    for line in subprocess.run(["readelf", "-SW", prog], capture_output=True, text=True, check=True).stdout.splitlines():
        parts = line.replace("[ ", "[").split()
        if len(parts) > 5 and parts[0].startswith("[") and parts[1].startswith("."):
            addr, size = int(parts[3], 16), int(parts[5], 16)
            if addr:
                out.append((addr, addr + size, parts[1]))
    return out


def symbols(prog):
    syms = []
    for line in subprocess.run(["nm", "-n", "-C", prog], capture_output=True, text=True, check=True).stdout.splitlines():
        parts = line.split(None, 2)
        if len(parts) == 3:
            syms.append((int(parts[0], 16), parts[2]))
    return syms


def main():
    prog, script = sys.argv[1], sys.argv[2]
    secs = sections(prog)
    syms = symbols(prog)
    addrs = [a for a, _ in syms]
    counts = {}
    for line in open(script):
        parts = line.split(None, 2)
        if len(parts) < 2:
            continue
        addr = int(parts[0], 16)
        ipsym = parts[2].strip() if len(parts) > 2 else "?"
        sec = next((n for lo, hi, n in secs if lo <= addr < hi), "other")
        sym = "-"
        if sec != "other":
            i = bisect.bisect_right(addrs, addr) - 1
            sym = syms[i][1][:70] if i >= 0 else "-"
        counts[sec] = counts.get(sec, 0) + 1
        print(f"{addr & ~0xfff:#014x} {sec:14} {sym:70} ip:{ipsym[:50]}")
    print("total", sum(counts.values()), " ".join(f"{k}={v}" for k, v in sorted(counts.items())))


if __name__ == "__main__":
    main()
