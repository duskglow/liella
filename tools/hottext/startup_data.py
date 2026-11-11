#!/usr/bin/env python3
"""STARTUP-DATA-WINDOWS (D-X.50): the data objects `page_faults true` touches at start-up.

usage: startup_data.py <linked probe> <isolated probe> <startup-text.keys> <profile> <startup-data.keys> [--replace]

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
Output (rewritten in place, the union over the profiles traced, like startup-text.keys):
  `<key> TAB <ro|rw> TAB <profiles>` lines, keys build-stable (tools/hottext/keys.py), sorted.
This profile's trace is the whole truth for it: an old key it did not see loses the profile and is
dropped when no profile is left.
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
            data.append((start, start + max(size, 1), n))
        elif t in "tTwWiI":
            funcs.setdefault(K.key(n), []).append((start, start + max(size, 1)))
    return sorted(data), funcs


def startup_fn_keys(path, profile):
    out = set()
    with open(path) as f:
        for line in f:
            p = line.rstrip("\n").split("\t")
            if line.startswith("#") or len(p) not in (3, 4):  # 4th column: provenance (D-X.61)
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
        name = min(hit, key=lambda o: o[1] - o[0])[2]
        if name.startswith(("anon.", ".L")):
            # A compiler-numbered constant has no build-stable name: the layout check finds it at check time
            # through the start-up functions' operands, which address it.
            continue
        out.setdefault(K.key(name), "rw" if sec[3] else "ro")
    return out


def main():
    args = [a for a in sys.argv[1:] if a != "--replace"]
    replace = "--replace" in sys.argv[1:]
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
    old = {}
    if not replace and os.path.exists(out_path):
        with open(out_path) as f:
            for line in f:
                p = line.rstrip("\n").split("\t")
                if line.startswith("#") or len(p) != 3:
                    continue
                old[p[0]] = (p[1], set(p[2].split(",")) - {"-"})
    merged = {}
    for k, (kind, profs) in old.items():
        profs = profs - {profile}
        if profs:
            merged[k] = (kind, profs)
    for k, kind in traced.items():
        merged[k] = (kind, merged.get(k, (kind, set()))[1] | {profile})
    with open(out_path + ".tmp", "w") as f:
        f.write("# @generated by tools/hottext/startup_data.py (tools/hottext/regen.sh); do not edit by hand.\n")
        f.write("# STARTUP-DATA-WINDOWS (layout/contracts.tsv, D-X.50): the data objects `page_faults true` touches at start-up\n")
        f.write("# (perf page-fault data addresses and the RIP-relative operands of the start-up functions of startup-text.keys).\n")
        f.write("# <key> TAB <ro|rw: the section's kind when traced> TAB <profiles traced in>.\n")
        for k in sorted(merged):
            kind, profs = merged[k]
            f.write(f"{k}\t{kind}\t{','.join(p for p in PROFILES if p in profs)}\n")
    os.replace(out_path + ".tmp", out_path)
    n = sum(1 for v in traced.values())
    print(f"startup-data: {n} objects traced ({profile}: {sum(v == 'ro' for v in traced.values())} ro), {len(merged)} in {out_path}")


if __name__ == "__main__":
    main()
