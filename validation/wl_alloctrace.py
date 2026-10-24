# gdb python script of validation/wl_alloctrace.sh (runs on keke, inside `gdb -batch -x`): traces the allocator entry
# points of ONE Liella program run and writes a summary to $TROUT. Liella binaries only (never glibc).
#   env: TROUT summary file; TRTOP rows per table (default 10); TRMIN smallest block whose realloc chain is followed
#   (default 256 bytes; chains starting from a smaller malloc are seen only from their first realloc on).
# Entry points are breakpoints on the exact entry address (*malloc), so the word at (rsp) is the return address.
# Return values are read only for realloc and for blocks >= TRMIN (a finish breakpoint costs far more than an entry).
import collections
import os
import time

import gdb

TOP = int(os.environ.get("TRTOP", "10"))
CHAINMIN = int(os.environ.get("TRMIN", "256"))
OUT = os.environ["TROUT"]
DEADLINE = time.time() + int(os.environ.get("TRSECS", "240"))   # the trace stops here and reports the calls seen so far
stopped = [False]
nstop = [0]

calls = collections.defaultdict(collections.Counter)    # op -> size class -> calls
nbytes = collections.defaultdict(collections.Counter)   # op -> size class -> requested bytes
sites = collections.defaultdict(collections.Counter)    # op -> return address -> calls
sitebytes = collections.defaultdict(collections.Counter)
siteinfo = {}                                           # return address -> "caller < parent < grandparent"
live = {}                                               # pointer -> realloc chain [(label, size, moved)]
chains = collections.Counter()                          # finished chain signature -> count
rstat = collections.Counter()                           # realloc: moved / in_place / failed / from_null / to_free
seq = [0]


def reg(r):
    return int(gdb.parse_and_eval("$" + r)) & 0xFFFFFFFFFFFFFFFF


def cls(sz):
    return 0 if sz == 0 else (sz - 1).bit_length()


def where():
    sp = reg("rsp")
    addr = int(gdb.parse_and_eval("*(unsigned long *)%d" % sp)) & 0xFFFFFFFFFFFFFFFF
    if addr not in siteinfo:
        names = []
        f = gdb.newest_frame()
        for _ in range(3):
            f = f.older() if f is not None else None
            if f is None:
                break
            names.append(f.name() or "%#x" % f.pc())
        siteinfo[addr] = " < ".join(names) or "%#x" % addr
    return addr


def finish_chain(ch, tail):
    parts = []
    for i, (lab, sz, moved) in enumerate(ch):
        if i == 0:
            parts.append("%s%s" % (lab, sz if sz is not None else ""))
        else:
            parts.append("%d %s" % (sz, "moved" if moved else "in-place"))
    n = len(parts) - 1
    if n < 1:
        return
    if len(parts) > 14:
        parts = parts[:7] + ["... (%d steps)" % n] + parts[-4:]
    chains[" -> ".join(parts) + tail] += 1


class Ret(gdb.FinishBreakpoint):
    def __init__(self, kind, sz, old, chain):
        super().__init__(gdb.newest_frame(), internal=True)
        self.silent = True
        self.kind, self.sz, self.old, self.chain = kind, sz, old, chain

    def stop(self):
        p = reg("rax")
        if self.kind == "realloc":
            if p == 0:
                rstat["failed"] += 1
                if self.chain is not None:
                    live[self.old] = self.chain
            else:
                moved = p != self.old
                rstat["moved" if moved else "in_place"] += 1
                if self.chain is not None:
                    if self.old:
                        self.chain.append(("", self.sz, moved))
                    live[p] = self.chain
        elif p:
            live[p] = [("m", self.sz, False)]
        return False

    def out_of_scope(self):
        pass


class Entry(gdb.Breakpoint):
    def __init__(self, name, sizereg):
        super().__init__("*" + name, internal=True)
        self.silent = True
        self.op, self.sizereg = name, sizereg

    def stop(self):
        op = self.op
        nstop[0] += 1
        if nstop[0] % 512 == 0 and time.time() > DEADLINE:
            stopped[0] = True
            return True
        if op == "free":
            p = reg("rdi")
            calls[op][0] += 1
            ch = live.pop(p, None)
            if ch is not None:
                finish_chain(ch, "")
            return False
        if op == "calloc":
            sz = reg("rdi") * reg("rsi")
        else:
            sz = reg(self.sizereg)
        c = cls(sz)
        calls[op][c] += 1
        nbytes[op][c] += sz
        addr = where()
        sites[op][addr] += 1
        sitebytes[op][addr] += sz
        if op == "realloc":
            old = reg("rdi")
            ch = live.pop(old, None) if old else None
            if old == 0:
                rstat["from_null"] += 1
            if sz == 0 and old:
                rstat["to_free"] += 1
                if ch is not None:
                    finish_chain(ch, " -> freed")
                return False
            if ch is None and (old or sz >= CHAINMIN):
                ch = [("m", sz, False)] if old == 0 else [("?", None, False)]
            Ret("realloc", sz, old, ch)
        elif op in ("malloc", "calloc") and sz >= CHAINMIN:
            Ret(op, sz, 0, None)
        return False


for _n, _r in (("malloc", "rdi"), ("calloc", "rdi"), ("realloc", "rsi"), ("free", "rdi"),
               ("memalign", "rsi"), ("aligned_alloc", "rsi"), ("posix_memalign", "rdx")):
    try:
        Entry(_n, _r)
    except gdb.error:
        pass

gdb.execute("run")

for ch in live.values():
    finish_chain(ch, " (live at exit)")


def label(c):
    return "0" if c == 0 else "<=%d" % (1 << c)


with open(OUT, "w") as out:
    if stopped[0]:
        out.write("!! TRUNCATED: the program was stopped after %d allocator calls (TRSECS time cap); every count below is a"
                  " prefix of the run, not the whole run\n" % nstop[0])
    else:
        out.write("traced the whole run: %d allocator calls\n" % nstop[0])
    ops = [o for o in ("malloc", "calloc", "realloc", "memalign", "aligned_alloc", "posix_memalign") if calls[o]]
    out.write("== calls by size class (class = request <= 2^k; calls / MB requested)\n")
    allc = sorted(set(c for o in ops for c in calls[o]))
    out.write("%-10s" % "class" + "".join("%18s" % o for o in ops) + "\n")
    for c in allc:
        out.write("%-10s" % label(c) + "".join(
            "%18s" % ("%d / %.2f" % (calls[o][c], nbytes[o][c] / 1e6) if calls[o][c] else "-") for o in ops) + "\n")
    out.write("%-10s" % "total" + "".join("%18s" % ("%d / %.2f" % (sum(calls[o].values()), sum(nbytes[o].values()) / 1e6))
                                            for o in ops) + "\n")
    out.write("free calls: %d\n" % sum(calls["free"].values()))
    out.write("== realloc: %s\n" % (", ".join("%s %d" % kv for kv in sorted(rstat.items())) or "none"))
    out.write("== realloc chains (blocks >= %d bytes at malloc, or any block once realloc'd; most calls first)\n" % CHAINMIN)
    out.write("   m<size> = malloc/calloc of that size, ? = block first seen at its first realloc\n")
    rows = sorted(chains.items(), key=lambda kv: -kv[1] * (kv[0].count("->") or 1))
    for sig, n in rows[:TOP]:
        out.write("%6d x  %s\n" % (n, sig))
    if len(rows) > TOP:
        out.write("   ... %d more distinct chains\n" % (len(rows) - TOP))
    for op in ops:
        out.write("== top call sites of %s (calls, MB requested, caller < parent < grandparent)\n" % op)
        for addr, n in sites[op].most_common(TOP):
            out.write("%8d %9.2f  %s\n" % (n, sitebytes[op][addr] / 1e6, siteinfo[addr]))
