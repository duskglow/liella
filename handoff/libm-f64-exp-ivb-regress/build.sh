#!/bin/bash
# Build cbench_libm (one harness source, today's) against each revision's cached products, plus glibc builds.
# Usage: bash build.sh <tree with bench/c and include> <outdir> <sha>...
set -eu
tree=$1; out=$2; shift 2
mkdir -p "$out"
src=$tree/bench/c/cbench_libm.c
for sha in "$@"; do
    p=~/wl/products/$sha
    cc -std=c17 -O2 -fno-builtin -static -nostdlib "$p/liella-crt1.o" -I "$tree/include" "$src" \
        "$p/libm.a" "$p/libc.a" -lgcc -o "$out/cbench_libm.${sha:0:8}"
    echo "built $out/cbench_libm.${sha:0:8}"
done
cc -std=c17 -O2 -fno-builtin "$src" -lm -o "$out/cbench_libm.host-dynamic"
cc -std=c17 -O2 -fno-builtin -static "$src" -lm -o "$out/cbench_libm.host-static"
ls -la "$out"
for b in "$out"/cbench_libm.*; do echo "== $b"; CBENCH_ROWS=exp,exp_small "$b"; done
