#!/bin/bash
# Interleaved timing of cbench_libm exp/exp_small: every program once per round, ROUNDS rounds, PASSES passes.
# Each program run prints its best-of-7 ns per call per row. Usage: bash run.sh <dir with cbench_libm.*> [rounds] [passes]
set -u
d=$1; rounds=${2:-7}; passes=${3:-2}
echo "host $(uname -n) cpu: $(grep -m1 'model name' /proc/cpuinfo | cut -d: -f2) load: $(cut -d' ' -f1-3 /proc/loadavg)"
for pass in $(seq 1 "$passes"); do
    for r in $(seq 1 "$rounds"); do
        for b in "$d"/cbench_libm.*; do
            CBENCH_ROWS=exp,exp_small "$b" | sed "s|^|pass=$pass round=$r prog=${b##*.} |"
        done
    done
done
echo "load at end: $(cut -d' ' -f1-3 /proc/loadavg)"
echo rc=0
