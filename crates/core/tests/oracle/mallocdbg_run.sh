#!/bin/sh
# Runs mallocdbg_diff (spec §3.2-3.3) in every mode, each under its environment.
# Usage: mallocdbg_run.sh PROGRAM SCRATCH [DEBUGLIB]
#   PROGRAM   the built probe
#   SCRATCH   an existing writable directory (the mtrace log is written there and removed)
#   DEBUGLIB  optional: the path of libc_malloc_debug.so; when given, a last part (after the line
#             `== reference only: LD_PRELOAD of the debug library`) runs the probe with it
#             preloaded. Only the reference is run that way: a static program ignores LD_PRELOAD.
# Every run's exit status is printed; a run's environment is printed as its header.
P=$1
S=$2
D=$3
[ -x "$P" ] && [ -d "$S" ] || { echo "usage: $0 PROGRAM SCRATCH [DEBUGLIB]" >&2; exit 2; }
ulimit -c 0
case $P in /*) ;; *) P=$(pwd)/$P ;; esac

run() { # run HEADER ENV... -- ARGS...
    h=$1; shift
    echo "#### $h"
    # the subshell's own report of a fatal signal ("Segmentation fault ...") is discarded; the
    # status shows the signal (128 + its number)
    ( env "$@" 2>&1 ) 2>/dev/null
    echo "#### exit status $?"
}

run "main" "$P" main
run "mcheck-early" "$P" mcheck-early
run "mcheck-null" "$P" mcheck-null
run "pedantic" "$P" pedantic
run "late" "$P" late
for h in i m f imf; do run "hooks-early $h" "$P" hooks-early $h; done
run "mtrace, MALLOC_TRACE unset" "$P" mtrace
run "mtrace, MALLOC_TRACE=<dir>/trace" MALLOC_TRACE="$S/trace" "$P" mtrace
rm -f "$S/trace"

for e in "" MALLOC_CHECK_=0 MALLOC_CHECK_=1 MALLOC_CHECK_=3 MALLOC_PERTURB_=165 MALLOC_PERTURB_=0 \
    MALLOC_MMAP_THRESHOLD_=32768 MALLOC_MMAP_MAX_=0 MALLOC_TOP_PAD_=4194304 \
    MALLOC_TRIM_THRESHOLD_=1000000000 MALLOC_ARENA_MAX=1 \
    GLIBC_TUNABLES=glibc.malloc.perturb=165 GLIBC_TUNABLES=glibc.malloc.mmap_threshold=32768 \
    GLIBC_TUNABLES=glibc.malloc.mmap_max=0 GLIBC_TUNABLES=glibc.malloc.top_pad=4194304 \
    GLIBC_TUNABLES=glibc.malloc.trim_threshold=1000000000 GLIBC_TUNABLES=glibc.malloc.tcache_count=0 \
    GLIBC_TUNABLES=glibc.malloc.check=3 GLIBC_TUNABLES=glibc.malloc.mxfast=0 \
    GLIBC_TUNABLES=glibc.malloc.nonexistent=1 GLIBC_TUNABLES=glibc.malloc.perturb=junk \
    GLIBC_TUNABLES=glibc.malloc.perturb=300 \
    GLIBC_TUNABLES=glibc.malloc.perturb=165:glibc.malloc.mmap_threshold=32768 \
    "GLIBC_TUNABLES=glibc.malloc.perturb=165 MALLOC_PERTURB_=1"; do
    # shellcheck disable=SC2086
    run "env ${e:-(nothing)}" $e "$P" env
done

[ -n "$D" ] || exit 0
echo "== reference only: LD_PRELOAD of the debug library"
L=LD_PRELOAD=$D
run "preloaded hooks-preloaded" "$L" "$P" hooks-preloaded
for h in i m f imf; do run "preloaded hooks-early $h" "$L" "$P" hooks-early $h; done
run "preloaded late" "$L" "$P" late
run "preloaded mcheck-early" "$L" "$P" mcheck-early
run "preloaded mcheck-null" "$L" "$P" mcheck-null
run "preloaded pedantic" "$L" "$P" pedantic
run "preloaded mtrace, MALLOC_TRACE unset" "$L" "$P" mtrace
run "preloaded mtrace, MALLOC_TRACE=<dir>/trace" "$L" MALLOC_TRACE="$S/trace" "$P" mtrace
run "preloaded mtrace, MALLOC_TRACE=<dir>/missing/trace" "$L" MALLOC_TRACE="$S/missing/trace" "$P" mtrace
rm -f "$S/trace"
for e in "" MALLOC_CHECK_=1 MALLOC_CHECK_=2 MALLOC_CHECK_=3 GLIBC_TUNABLES=glibc.malloc.check=3; do
    # shellcheck disable=SC2086
    run "preloaded env ${e:-(nothing)}" "$L" $e "$P" env
done
