# Hot-text tracer (PAGEFAULTS-2): run under `gdb -batch -x trace_gdb.py --args <static program> ...`.
# Puts a one-shot breakpoint on every function of OUR static binary, runs it, and writes the names of
# the functions that executed (every name at each hit address) to $HOT_TEXT_OUT, one per line, in
# first-touch order (HOT-ORDER: the list keeps that order and the link lays the group out by it).
# Liella products only (the binary is linked against libc.a); never point it at glibc.
import os
import subprocess

import gdb

binp = gdb.current_progspace().filename
out_path = os.environ["HOT_TEXT_OUT"]
stdin_path = os.environ.get("HOT_TEXT_STDIN", "/dev/null")

names = {}
nm = subprocess.run(["nm", "--defined-only", binp], capture_output=True, text=True, check=True).stdout
for line in nm.splitlines():
    p = line.split()
    if len(p) == 3 and p[1] in "tTwWiI":
        names.setdefault(int(p[0], 16), []).append(p[2])

for cmd in ("set confirm off", "set pagination off", "set breakpoint always-inserted on", "set startup-with-shell off"):
    gdb.execute(cmd)
# Stop at the entry first (breakpoints on a static non-PIE binary can be placed before `run`).
by_bp = {}
for addr in names:
    bp = gdb.Breakpoint("*0x%x" % addr, internal=True, temporary=True)
    bp.silent = True
    by_bp[bp.number] = addr

hit = []
# With startup-with-shell off, gdb ignores `run < file > file` (HOT-ORDER-3: p_cat read an empty stdin
# and never reached fwrite), so the inferior inherits gdb's own stdin/stdout: point them at the files.
for fd, path, mode in ((0, stdin_path, os.O_RDONLY), (1, "/dev/null", os.O_WRONLY)):
    f = os.open(path, mode)
    os.dup2(f, fd)
    os.close(f)
gdb.execute("run", to_string=True)
while True:
    try:
        frame = gdb.selected_frame()
    except gdb.error:
        break
    hit.append(int(frame.pc()))  # one-shot breakpoints: each address stops once, in touch order
    try:
        gdb.execute("continue", to_string=True)
    except gdb.error:
        break

with open(out_path, "w") as f:
    for addr in hit:
        for n in names.get(addr, []):
            f.write(n + "\n")
