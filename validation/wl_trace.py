# wl_trace.py: exact instruction split of ONE call of a function in a Liella-linked binary (gdb single-step).
# Run by wl_percall_run.sh (via wl_trace.sh): gdb -batch -nx -x wl_trace.py --args <pc> <case> <n> <dir>
# Env: WL_TRACE_FUNC (symbol to break on), WL_TRACE_SKIP (hits to let pass first: warm caches, lazy init),
# WL_TRACE_MAX (step limit). Steps from the function's entry until it returns (stack pointer above the entry
# value), counting every user-space instruction by the function it executes in (a syscall counts as one).
# Clean room: only ever run on OUR products; wl_percall_run.sh never passes it a glibc binary.
import collections
import os
import subprocess

import gdb

func = os.environ.get("WL_TRACE_FUNC", "")
skip = int(os.environ.get("WL_TRACE_SKIP", "10"))
limit = int(os.environ.get("WL_TRACE_MAX", "200000"))

gdb.execute("set pagination off")
gdb.execute("set confirm off")
gdb.execute("set breakpoint pending off")


def elf_symbols():
    """name -> address and address -> names of the binary's code symbols (nm, the linked ELF symbol table)."""
    by_name, by_addr = {}, collections.defaultdict(list)
    try:
        out = subprocess.run(["nm", "--defined-only", gdb.current_progspace().filename],
                             capture_output=True, text=True, timeout=60).stdout
    except (OSError, subprocess.SubprocessError):
        return by_name, by_addr
    for line in out.splitlines():
        f = line.split()
        if len(f) == 3 and f[1] in "TtWwi":
            a = int(f[0], 16)
            by_name.setdefault(f[2], a)
            by_addr[a].append(f[2])
    return by_name, by_addr


# Break on the LINKED address of the ELF symbol, not on gdb's resolution of the name: an exported name can also be
# a debug-info function whose code the link never calls (an alias whose body is another symbol, an asm entry), and
# gdb then breaks at an address that never runs (TOOLS-9: fprintf). The fortified entry __<func>_chk (gcc -O2 with
# _FORTIFY_SOURCE turns fprintf(...) into __fprintf_chk(...)) gets a breakpoint too: the first one hit is traced.
by_name, by_addr = elf_symbols()
addr = by_name.get(func)
aliases = sorted(n for n in by_addr.get(addr, []) if n != func) if addr is not None else []
chk = "__%s_chk" % func
targets = [(func, addr)] + ([(chk, by_name[chk])] if chk in by_name and by_name[chk] != addr else [])
try:
    for name, a in targets:
        gdb.execute(("break *0x%x" % a) if a is not None else ("break %s" % name), to_string=True)
        print("wl_trace: breaking at %s (ELF symbol %s%s)" % ("0x%x" % a if a is not None else "gdb's " + name, name,
              "; same address: " + ", ".join(aliases[:6]) if name == func and aliases else ""))
except gdb.error as e:
    print("wl_trace: no symbol %s in the binary (%s)" % (func, e))
    gdb.execute("quit 2")
if skip > 0:
    for b in gdb.breakpoints():
        b.ignore_count = skip
gdb.execute("run %s > /dev/null" % gdb.parameter("args"), to_string=True)  # "run" alone with a redirection would drop --args
if not gdb.selected_inferior().pid:
    print("wl_trace: %s was called %d times or fewer in this case; nothing traced" % (" or ".join(n for n, _ in targets), skip))
    if addr is None:
        print("wl_trace: %s is not an ELF code symbol of the binary; gdb resolved it from debug info" % func)
    else:
        print("wl_trace: the compiler may have rewritten the call (printf -> puts, fprintf -> fwrite/fputc); "
              "check the case's calls with: nm <binary> | grep -w U, or trace the rewritten name")
    gdb.execute("quit 2")
hit = int(gdb.parse_and_eval("$pc"))
for name, a in targets:
    if a == hit and name != func:
        print("wl_trace: %s is reached as %s here (fortified entry); tracing that call" % (func, name))
        func = name
gdb.execute("delete", to_string=True)

sp0 = int(gdb.parse_and_eval("$sp"))
counts = collections.Counter()
order = []
n = 0
while n < limit:
    try:
        name = gdb.selected_frame().name() or hex(int(gdb.parse_and_eval("$pc")))
    except gdb.error:
        name = "?"
    counts[name] += 1
    if order and order[-1][0] == name:
        order[-1][1] += 1
    else:
        order.append([name, 1])
    n += 1
    gdb.execute("stepi", to_string=True)
    if int(gdb.parse_and_eval("$sp")) > sp0:
        break
print("TOTAL %d instructions in call %d of %s%s" % (n, skip + 1, func, " (STEP LIMIT REACHED)" if n >= limit else ""))
for name, c in counts.most_common(40):
    print("%8d  %5.1f%%  %s" % (c, 100.0 * c / n, name[:120]))
if len(counts) > 40:
    print("   ... %d more functions" % (len(counts) - 40))
print("SEQUENCE (function: consecutive instructions), first 60 runs:")
print(" > ".join("%s:%d" % (f[:50], c) for f, c in order[:60]))
gdb.execute("kill", to_string=True)
