#!/usr/bin/env python3
"""STARTUP-DATA-WINDOWS (D-X.50): the data objects `page_faults true` touches at start-up.

usage: startup_data.py <linked probe> <isolated probe> <startup-text.keys> <profile> <startup-data.keys>

<linked probe>: crates/core/tests/oracle/page_faults.c linked statically against the products as
crates/core/tests/hot_text_products.rs links it (tools/hottext/startup_data.sh does that).
<isolated probe>: the same link with every input section of `.data`, `.data1`, `.bss` and `.lbss` on a
64 KiB boundary of its own (the default linker script with SUBALIGN(65536) on those output sections).
Three sources, all without a debugger watch:
  1. the page faults of `<probe> true` with their data addresses (`perf record -e page-faults -d`):
     each names the first object touched on a page (a window, for read-only file pages);
  2. the same faults of `<isolated probe> true`, writable sections only: no two sections share a page
     or a fault-around window there, so every writable object start-up reads or writes faults at its
     own address and is keyed by access, not only when it happens to be first on its page in this
     build's `.data` order (LAYOUT-DATA-CLUSTERS-3, D-X.67: `__libc_single_threaded` and
     `_IO_2_1_stdout_` sat on pages another object faulted first, so they had no key and moved with
     the order of `.data`). Objects that share one input section (a C file's `.data`) still count once;
  3. every RIP-relative operand of the traced start-up functions (the keys of <startup-text.keys> for
     <profile>; `objdump -d` prints its target as `# <addr> <sym>`): the objects their code addresses.
Each address is resolved to the data symbol that holds it (`nm -S`); text, TLS, unnamed bytes and
compiler-numbered constants (`anon.<hash>.<n>`: no build-stable name) are left out; the layout check
counts those through the start-up functions' operands at check time. The kind is the section's: `rw` when the
section is writable (.data, .bss, .lbss, .data.rel.ro, ...), else `ro`.
Output (written fresh by every trace-at-build `cargo xtask so`, LT-5: nothing of an earlier build is kept):
  `<key> TAB <ro|rw> TAB <profile>` lines, keys build-stable (tools/hottext/keys.py), sorted.
A host where perf cannot record (perf_event_paranoid > 2, no perf) fails: the operand scan alone
misses objects reached through pointers.
"""
import os
import re
import subprocess
import sys
import tempfile

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import keys as K  # noqa: E402

PROFILES = ["fast", "release"]
DATA_TYPES = set("dDbBrRvVgGsS")


def run(args, **kw):
    r = subprocess.run(args, capture_output=True, text=True, **kw)
    if r.returncode != 0:
        sys.exit(f"startup_data: {' '.join(args)} failed ({r.returncode}): {r.stderr.strip()[-400:]}")
    return r.stdout


def sections(exe):
    """[(start, end, name, writable, tls)] of the allocated sections of the linked probe."""
    out = []
    for line in run(["readelf", "-SW", exe]).splitlines():
        m = re.match(r"\s*\[\s*\d+\]\s+(\S+)\s+\S+\s+([0-9a-f]+)\s+[0-9a-f]+\s+([0-9a-f]+)\s+\S+\s+(\S*)", line)
        # TLS is left out: a TLS symbol's value is a TLS-block offset, and .tbss overlaps the next section.
        if not m or "A" not in m.group(4) or "T" in m.group(4):
            continue
        start, size = int(m.group(2), 16), int(m.group(3), 16)
        out.append((start, start + size, m.group(1), "W" in m.group(4), "T" in m.group(4), "X" in m.group(4)))
    return out


def symbols(exe):
    """(data objects [(start, end, name)], functions {key: [(start, end)]}) of the probe."""
    data, funcs = [], {}
    for line in run(["nm", "-S", "--defined-only", exe]).splitlines():
        f = line.split()
        if len(f) == 4:
            a, s, t, n = f
        elif len(f) == 3:
            (a, t, n), s = f, "0"
        else:
            continue
        start, size = int(a, 16), int(s, 16)
        if t in DATA_TYPES:
            # The 4th field: nm gave a size (a linker marker such as __bss_start or _edata has none).
            data.append((start, start + max(size, 1), n, size > 0))
        elif t in "tTwWiI":
            funcs.setdefault(K.key(n), []).append((start, start + max(size, 1)))
    return sorted(data), funcs


def startup_fn_keys(path, profile):
    out = set()
    with open(path) as f:
        for line in f:
            p = line.rstrip("\n").split("\t")
            if line.startswith("#") or len(p) != 3:
                continue
            if profile in p[2].split(","):
                out.add(p[0])
    return out


def operand_targets(exe, ranges):
    """The `# <addr>` targets of the instructions inside `ranges` ([(start, end)])."""
    out = []
    for lo, hi in sorted(ranges):
        text = run(["objdump", "-d", "--no-show-raw-insn", f"--start-address={lo:#x}", f"--stop-address={hi:#x}", exe])
        for m in re.finditer(r"#\s+([0-9a-f]+)\b", text):
            out.append(int(m.group(1), 16))
    return out


def fault_addrs(exe):
    with tempfile.TemporaryDirectory() as d:
        data = os.path.join(d, "perf.data")
        run(["perf", "record", "-q", "-e", "page-faults", "-c", "1", "-d", "-o", data, "--", exe, "true"])
        text = run(["perf", "script", "-F", "addr", "-i", data])
    return [int(t, 16) for t in text.split() if re.fullmatch(r"[0-9a-f]+", t)]


def resolve(addrs, data, secs, writable_only=False):
    """{key: kind} of the data objects holding `addrs` (text, TLS and unnamed bytes skipped)."""
    out = {}
    for a in addrs:
        sec = next((s for s in secs if s[0] <= a < s[1]), None)
        if sec is None or sec[4] or sec[5] or (writable_only and not sec[3]):
            continue
        hit = [o for o in data if o[0] <= a < o[1]]
        if not hit:
            continue
        # The smallest object holding the address, by every name it has: a linker marker of no size
        # (__bss_start at the first .bss object) never stands in for the object, and each alias is
        # keyed (__progname and program_invocation_name: the move matches whichever names the section).
        sized = [o for o in hit if o[3]] or hit
        least = min(o[1] - o[0] for o in sized)
        for o in sized:
            if o[1] - o[0] != least or o[2].startswith(("anon.", ".L")):
                # A compiler-numbered constant has no build-stable name: the layout check finds it at check time
                # through the start-up functions' operands, which address it.
                continue
            out.setdefault(K.key(o[2]), "rw" if sec[3] else "ro")
    return out


def main():
    args = sys.argv[1:]
    if len(args) != 5 or args[3] not in PROFILES:
        sys.exit(__doc__)
    exe, iso, text_keys, profile, out_path = args
    exe, iso = os.path.abspath(exe), os.path.abspath(iso)
    secs = sections(exe)
    data, funcs = symbols(exe)
    want = startup_fn_keys(text_keys, profile)
    ranges = [r for k in want for r in funcs.get(k, [])]
    if not ranges:
        sys.exit(f"startup_data: no function of {text_keys} ({profile}) in {exe}")
    traced = resolve(fault_addrs(exe) + operand_targets(exe, ranges), data, secs)
    iso_data, _ = symbols(iso)
    for k, kind in resolve(fault_addrs(iso), iso_data, sections(iso), writable_only=True).items():
        traced.setdefault(k, kind)
    with open(out_path + ".tmp", "w") as f:
        f.write("# Written by tools/hottext/startup_data.py during cargo xtask so (a trace of this build; never committed).\n")
        f.write("# STARTUP-DATA-WINDOWS (layout/contracts.tsv, D-X.50): the data objects `page_faults true` touches at start-up\n")
        f.write("# (perf page-fault data addresses and the RIP-relative operands of the start-up functions of startup-text.keys).\n")
        f.write("# <key> TAB <ro|rw: the section's kind when traced> TAB <profile traced>.\n")
        for k in sorted(traced):
            f.write(f"{k}\t{traced[k]}\t{profile}\n")
    os.replace(out_path + ".tmp", out_path)
    print(f"startup-data: {len(traced)} objects traced ({profile}: {sum(v == 'ro' for v in traced.values())} ro) into {out_path}")


if __name__ == "__main__":
    main()
