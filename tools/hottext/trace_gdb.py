# Hot-text tracer (PAGEFAULTS-2): run under `gdb -batch -x trace_gdb.py --args <static program> ...`.
# Puts a one-shot breakpoint on every function of OUR static binary, runs it, and writes the names of
# the functions that executed (every name at each hit address) to $HOT_TEXT_OUT, one per line, in
# first-touch order (HOT-ORDER: the list keeps that order and the link lays the group out by it).
# Liella products only (the binary is linked against libc.a); never point it at glibc.
#
# LAYOUT-TRACE-HOST-CANONICAL (LT-4): the trace is a function of the program, not of the host.
#  * CPU: the program never sees the host's CPU. Its two CPU entries (`liella_machine_cpuid`,
#    `liella_machine_xgetbv0`; every CPUID / XCR0 read of the library goes through them) are answered
#    by the tracer from a fixed virtual CPU, and the program runs once per CPU class (CLASSES below:
#    every combination of the two decisions that choose code, AVX2 and ERMS). The runs are merged into
#    one first-touch order (merge()): the first class's order, then every function only another class
#    ran, right after the function that class ran before it. So every candidate a class decision can
#    pick is in the list, whichever the build host would have picked.
#  * Environment: the program's environment is exactly the HOT_TEXT_ENV_<NAME>=<value> variables
#    given to gdb, as NAME=value in name order (nothing of the host's or gdb's own environment);
#    cwd `/`; address randomisation off; stdin $HOT_TEXT_STDIN, stdout /dev/null; the program runs
#    from a copy at a path of fixed length (argv[0]), so the work directory's path does not reach it.
#  HOT_TEXT_CLASSES (comma list of CLASSES names, default all) picks the classes; `host` alone runs the
#  host's own CPU (no virtual CPU: a diagnosis aid, never a list the build groups).
import os
import shutil
import struct
import subprocess
import sys
import tempfile

import gdb

binp = gdb.current_progspace().filename
out_path = os.environ["HOT_TEXT_OUT"]
stdin_path = os.environ.get("HOT_TEXT_STDIN", "/dev/null")


def fail(msg):
    sys.stderr.write("trace_gdb: %s\n" % msg)
    sys.stderr.flush()
    os._exit(2)


# The virtual CPUs (Intel SDM vol. 2A, CPUID; bit numbers as crates/rt/src/cpu.rs reads them).
L1D_BASE = (1 << 0) | (1 << 4) | (1 << 8) | (1 << 15) | (1 << 23) | (1 << 24) | (1 << 25) | (1 << 26)  # FPU TSC CX8 CMOV MMX FXSR SSE SSE2
L1C_V3 = (1 << 0) | (1 << 9) | (1 << 12) | (1 << 13) | (1 << 19) | (1 << 20) | (1 << 22) | (1 << 23) | (1 << 26) | (1 << 27) | (1 << 28) | (1 << 29)
L7B_V3 = (1 << 3) | (1 << 5) | (1 << 8)  # BMI1 AVX2 BMI2
L7B_ERMS = 1 << 9
E1C_V3 = (1 << 0) | (1 << 5)  # LAHF LZCNT
E1D = (1 << 11) | (1 << 20) | (1 << 29)  # SYSCALL NX LM
# name -> (leaf 1 ECX, leaf 7 EBX, 80000001h ECX, XCR0, XSAVE area size)
CLASSES = {
    "v3-erms": (L1C_V3, L7B_V3 | L7B_ERMS, E1C_V3, 0x7, 832),
    "v3": (L1C_V3, L7B_V3, E1C_V3, 0x7, 832),
    "base-erms": (0, L7B_ERMS, 0, 0, 0),
    "base": (0, 0, 0, 0, 0),
}
# The host features a class's code paths execute (lscpu / /proc/cpuinfo flag names).
NEEDS = {"v3-erms": ("avx", "avx2", "fma", "bmi1", "bmi2", "movbe", "f16c", "abm"), "v3": ("avx", "avx2", "fma", "bmi1", "bmi2", "movbe", "f16c", "abm")}


def cpuid(cls, leaf, sub):
    l1c, l7b, e1c, xcr0, xsize = CLASSES[cls]
    if leaf == 0:
        return (0xD, 0x756E6547, 0x6C65746E, 0x49656E69)  # max leaf 0xd, "GenuineIntel"
    if leaf == 1:
        return (0x000306F0, 0x00010800, l1c, L1D_BASE)
    if leaf == 7 and sub == 0:
        return (0, l7b, 0, 0)
    if leaf == 0xD and sub == 0 and xcr0:
        return (xcr0, xsize, xsize, 0)
    if leaf == 0x80000000:
        return (0x80000008, 0, 0, 0)
    if leaf == 0x80000001:
        return (0, 0, e1c, E1D)
    return (0, 0, 0, 0)


classes = [c for c in os.environ.get("HOT_TEXT_CLASSES", ",".join(CLASSES)).split(",") if c]
virtual = classes != ["host"]
if virtual:
    for c in classes:
        if c not in CLASSES:
            fail("HOT_TEXT_CLASSES: unknown class %r (classes: %s, or host)" % (c, " ".join(CLASSES)))
    with open("/proc/cpuinfo") as f:
        flags = set(next((ln.split(":", 1)[1].split() for ln in f if ln.startswith("flags")), []))
    for c in classes:
        missing = [n for n in NEEDS.get(c, ()) if n not in flags]
        if missing:
            fail("class %s runs code this host cannot execute (missing %s): trace on an x86-64-v3 host" % (c, " ".join(missing)))

names = {}
nm = subprocess.run(["nm", "--defined-only", binp], capture_output=True, text=True, check=True).stdout
for line in nm.splitlines():
    p = line.split()
    if len(p) == 3 and p[1] in "tTwWiI":
        names.setdefault(int(p[0], 16), []).append(p[2])
by_name = {n: a for a, ns in names.items() for n in ns}
emul = {}
if virtual:
    for n, kind in (("liella_machine_cpuid", "cpuid"), ("liella_machine_xgetbv0", "xgetbv")):
        if n not in by_name:
            fail("%s: no %s: the virtual CPU cannot answer for it" % (binp, n))
        emul[by_name[n]] = kind

# The program's copy at a fixed-length path (argv[0] is the path gdb runs).
stage = tempfile.mkdtemp(prefix="liella-trace.", dir="/tmp")
prog = os.path.join(stage, "p")
shutil.copy(binp, prog)
env = sorted((k[len("HOT_TEXT_ENV_"):], v) for k, v in os.environ.items() if k.startswith("HOT_TEXT_ENV_"))
for cmd in ["set confirm off", "set pagination off", "set breakpoint always-inserted on", "set startup-with-shell off",
            "set disable-randomization on", "file " + prog, "set cwd /", "unset environment"] + \
           ["set environment %s=%s" % kv for kv in env]:
    gdb.execute(cmd, to_string=True)
# HOT_TEXT_FOLLOW_FORK=child (TIMING-COV-FORK-COW-FAULTS): trace the parent up to its first fork,
# then the child to its exit. `fork` does not copy the page tables of file-backed text, so a fork
# child faults on every 64 KiB text window its path touches; the parent's later path is not traced.
if os.environ.get("HOT_TEXT_FOLLOW_FORK") == "child":
    gdb.execute("set follow-fork-mode child")
# With startup-with-shell off, gdb ignores `run < file > file` (HOT-ORDER-3: p_cat read an empty stdin
# and never reached fwrite), so the inferior inherits gdb's own stdin/stdout: point them at the files.
for fd, path, mode in ((0, stdin_path, os.O_RDONLY), (1, "/dev/null", os.O_WRONLY)):
    f = os.open(path, mode)
    os.dup2(f, fd)
    os.close(f)


def reg(name):
    return int(gdb.selected_frame().read_register(name)) & 0xFFFFFFFFFFFFFFFF


def answer(kind, cls):
    """Return from the CPU entry at pc with the virtual CPU's answer (its own code never runs)."""
    inf = gdb.selected_inferior()
    if kind == "cpuid":
        regs = cpuid(cls, reg("rdi") & 0xFFFFFFFF, reg("rsi") & 0xFFFFFFFF)
        inf.write_memory(reg("rdx"), struct.pack("<4I", *regs))
    else:
        gdb.execute("set $rax = %d" % CLASSES[cls][3])
    sp = reg("rsp")
    ret = struct.unpack("<Q", bytes(inf.read_memory(sp, 8)))[0]
    gdb.execute("set $rsp = %d" % (sp + 8))
    gdb.execute("set $pc = %d" % ret)


# HOT_TEXT_REDIRECT (canon_host.sh): `<host path>=<file>` joined by `:`. At the entry of each path
# system call below the tracer points the path argument at the file (copied below the stack pointer).
redirect = {}
for item in filter(None, os.environ.get("HOT_TEXT_REDIRECT", "").split(":")):
    k, _, v = item.partition("=")
    redirect[k.encode()] = v.encode() + b"\0"
PATH_ARG = {2: "rdi", 4: "rdi", 6: "rdi", 21: "rdi", 257: "rsi", 262: "rsi", 269: "rsi", 332: "rsi", 439: "rsi"}
ENOSYS_ENTRY = (1 << 64) - 38  # %rax at a system call's entry stop


def redirect_path():
    """At a path system call's entry stop: point its path at the tree's file when redirected."""
    if reg("rax") != ENOSYS_ENTRY:
        return
    r = PATH_ARG.get(reg("orig_rax"))
    if r is None:
        return
    inf = gdb.selected_inferior()
    p = bytes(inf.read_memory(reg(r), 256)).split(b"\0", 1)[0]
    if p in redirect:
        new = redirect[p]
        at = (reg("rsp") - 4096 - len(new)) & ~15  # below the red zone, in the mapped stack
        inf.write_memory(at, new)
        gdb.execute("set $%s = %d" % (r, at))


def run_one(cls):
    """One run of the program on class `cls`: the function start addresses in first-touch order."""
    made = []
    for addr in names:
        if addr in emul and cls != "host":
            continue
        bp = gdb.Breakpoint("*0x%x" % addr, internal=True, temporary=True)
        bp.silent = True
        made.append(bp)
    if cls != "host":
        for addr in emul:
            bp = gdb.Breakpoint("*0x%x" % addr, internal=True)
            bp.silent = True
            made.append(bp)
    if redirect:
        gdb.execute("catch syscall open stat lstat access openat newfstatat faccessat statx faccessat2", to_string=True)
    hit, seen = [], set()
    gdb.execute("run", to_string=True)
    while True:
        try:
            pc = int(gdb.selected_frame().pc())
        except gdb.error:
            break
        if pc not in names and pc not in emul:
            redirect_path()  # a system call stop, not a function
        elif pc not in seen:  # one-shot breakpoints stop once each; the CPU entries stop at every call
            seen.add(pc)
            hit.append(pc)
        if pc in emul and cls != "host":
            answer(emul[pc], cls)
        try:
            gdb.execute("continue", to_string=True)
        except gdb.error:
            break
    for bp in made:
        if bp.is_valid():
            bp.delete()
    for bp in gdb.breakpoints() or ():  # the catchpoint
        bp.delete()
    return hit


def merge(runs):
    """The first run's order; a function only a later run touched goes right after the function that
    run touched before it (or first, when it is that run's first), in that run's order."""
    order = list(runs[0])
    at = {a: i for i, a in enumerate(order)}
    for run in runs[1:]:
        prev = None
        for a in run:
            if a not in at:
                i = 0 if prev is None else order.index(prev) + 1
                order.insert(i, a)
                at = {x: k for k, x in enumerate(order)}
            prev = a
    return order


runs = [run_one(c) for c in classes]
shutil.rmtree(stage, ignore_errors=True)
with open(out_path, "w") as f:
    for addr in merge(runs):
        for n in names.get(addr, []):
            f.write(n + "\n")
