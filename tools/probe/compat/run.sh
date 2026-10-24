#!/bin/sh
# LEGACY-PROBE-COMPAT (LG-0b): build and run the compat-version probes against the host C library and
# print the record (spec/behaviour/compat.glibc.txt). Usage: run.sh SCRATCH-DIR (created fresh, must not exist).
# Each probe runs in its own empty subdirectory with an empty environment (PATH only), 60 s timeout.
set -eu
here=$(cd "$(dirname "$0")" && pwd)
scratch=${1:?usage: run.sh SCRATCH-DIR}
mkdir "$scratch"
for p in compat_libc compat_thread compat_libm compat_lg2 compat_condvar; do
    cc -std=gnu17 -O1 -fno-builtin -pthread -o "$scratch/$p" "$here/$p.c" -lm 2>"$scratch/$p.cc.log" || { echo "== $p compile failed"; cat "$scratch/$p.cc.log"; exit 1; }
    mkdir "$scratch/$p.wd"
    echo "== $p"
    rc=0
    (cd "$scratch/$p.wd" && env -i PATH=/usr/bin:/bin LC_ALL=C timeout 60 "../$p") || rc=$?
    [ "$rc" = 0 ] || echo "== $p exit status $rc"
    chmod -R u+rwx "$scratch/$p.wd"
done
