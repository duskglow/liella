#!/usr/bin/env python3
"""Record the reference glibc's answers for sysconf, confstr and pathconf (spec §24.4).

Run on the reference system (the glibc that spec/abi was extracted from, see
spec/abi/PROVENANCE). For every _SC_* and _CS_* name in spec/abi/constants.tsv it records what
glibc returns, and for every _PC_* name the answer on a few file systems. Values that depend on
the machine (processor counts, memory, caches, limits) are recorded too, flagged by the second
run under changed limits; spec/abi/sysconf.tsv is the reference for the constant ones, and the
tests compare the rest with the live host.

Output: spec/abi/sysconf.tsv (kind, name, number, value, errno, varies).
"""
import os, subprocess, sys, tempfile

HERE = os.path.dirname(os.path.abspath(__file__))
ABI = os.path.join(HERE, "..", "abi")


def names(prefix):
    out = []
    for line in open(os.path.join(ABI, "constants.tsv"), encoding="utf-8"):
        f = line.rstrip("\n").split("\t")
        if len(f) == 2 and f[0].startswith(prefix):
            out.append((f[0], int(f[1])))
    return sorted(out, key=lambda x: (x[1], x[0]))


PATHS = ["/", "/proc", "/dev/null", "/tmp"]


def probe(tmp):
    sc, cs, pc = names("_SC_"), names("_CS_"), names("_PC_")
    src = ['#define _GNU_SOURCE 1', '#include <unistd.h>', '#include <stdio.h>', '#include <errno.h>',
           '#include <stdlib.h>', '#include <string.h>', 'int main(void){', ' char buf[4096]; long v; size_t n;']
    for name, num in sc:
        src.append(f' errno = 0; v = sysconf({num}); printf("sc\\t{name}\\t{num}\\t%ld\\t%d\\n", v, errno);')
    for name, num in cs:
        src.append(f' errno = 0; n = confstr({num}, buf, sizeof buf);'
                   f' if (n == 0) printf("cs\\t{name}\\t{num}\\t\\t%d\\n", errno);'
                   f' else printf("cs\\t{name}\\t{num}\\t%s\\t%d\\n", buf, errno);')
    for p in PATHS:
        for name, num in pc:
            src.append(f' errno = 0; v = pathconf("{p}", {num}); printf("pc:{p}\\t{name}\\t{num}\\t%ld\\t%d\\n", v, errno);')
    src.append(' return 0; }')
    c = os.path.join(tmp, "probe.c")
    open(c, "w").write("\n".join(src) + "\n")
    exe = c[:-2]
    subprocess.run(["gcc", "-std=gnu17", "-w", c, "-o", exe], check=True)
    return exe


def run(exe, nofile=None):
    pre = None
    if nofile:
        import resource
        def pre():
            resource.setrlimit(resource.RLIMIT_NOFILE, (nofile, nofile))
            resource.setrlimit(resource.RLIMIT_STACK, (8 << 20, resource.RLIM_INFINITY))
            resource.setrlimit(resource.RLIMIT_SIGPENDING, (77, 77))
            resource.setrlimit(resource.RLIMIT_NPROC, (66, 66))
    out = subprocess.run([exe], capture_output=True, text=True, check=True, preexec_fn=pre).stdout
    return [l.split("\t") for l in out.splitlines()]


def main():
    with tempfile.TemporaryDirectory() as tmp:
        exe = probe(tmp)
        a = run(exe)
        b = run(exe, nofile=99)
    varies = {(r[0], r[1]) for r, s in zip(a, b) if r[3] != s[3]}
    with open(os.path.join(ABI, "sysconf.tsv"), "w", newline="\n", encoding="utf-8") as f:
        f.write("kind\tname\tnumber\tvalue\terrno\tvaries\n")
        for r in a:
            f.write("\t".join(r + ["1" if (r[0], r[1]) in varies else "0"]) + "\n")
    print(f"{len(a)} rows, {len(varies)} vary with the limits")


if __name__ == "__main__":
    sys.exit(main())
