#!/bin/bash
# LAB-ONLY: this script drives the project's own test hosts and is not needed to build or test Liella.
# Exact per-function instruction split of ONE libc call in Liella (TOOLS-PERCALL; was STRCOLL-3/STDIO-3 trace.py).
# gdb single-steps one call of FUNCTION inside a wl_percall case, Liella only (never glibc), on keke, and prints
# the instructions executed per function (most first) and the call sequence. Use it after wl_percall.sh named
# the gap, to see WHERE inside the call the instructions go.
#
#   validation/wl_trace.sh [--spec FILE.c] [--skip K] [--max N] <dir|rev> <case> <function>
#
# <dir|rev>, --spec, --rev: as wl_percall.sh (runs on the host where it is started; from a role directory:
#   ~/bin/jobrun <label> count -- sh -c 'cd ~/w/<role> && bash validation/wl_trace.sh . strcoll_short strcoll') (the built-in cases by default; same products and binaries).
# <case>: a wl_percall case (strcoll_short, fgets_line, ...). <function>: a symbol of the Liella binary to
#   break on (an exported name such as strcoll or fgets, or an internal symbol).
# --skip K: trace call K+1 (default 10: past first-call initialisation). --max N: step limit (default 200000;
#   gdb steps ~10-20k instructions per second). Runs locally, no queue of its own (jobrun is the queue).
# The breakpoint is the ELF symbol's linked address (nm), so an exported alias whose body is another symbol is traced;
# a function never hit is reported with its same-address aliases ("X is an alias of Y; try Y") or a rewrite hint.
# Example: cd ~/w/<role> && bash validation/wl_trace.sh . strcoll_short strcoll
set -u
here=$(cd "$(dirname "$0")" && pwd)
skip=10; max=200000; pass=()
while [ $# -gt 0 ]; do
    case $1 in
        --spec) pass+=(--spec "$2"); shift 2 ;;
        --skip) skip=$2; shift 2 ;;
        --max) max=$2; shift 2 ;;
        --rev) pass+=(--rev "$2"); shift 2 ;;
        --nowait) shift ;;   # ignored: jobrun detaches
        -h|--help) sed -n '2,/^set -u/p' "$0" | sed '$d'; exit 0 ;;
        --) shift; break ;;
        -*) echo "wl_trace: unknown flag $1 (see --help)" >&2; exit 2 ;;
        *) break ;;
    esac
done
[ $# -eq 3 ] || { sed -n '2,/^set -u/p' "$0" | sed '$d'; exit 2; }
case "$skip$max" in *[!0-9]*) echo "wl_trace: --skip and --max take numbers" >&2; exit 2 ;; esac
case "$3" in *[!A-Za-z0-9_.$]*|"") echo "wl_trace: bad function name $3" >&2; exit 2 ;; esac
exec bash "$here/wl_percall.sh" "${pass[@]}" --cases "$2" --trace "$3:$skip:$max" "$1"
