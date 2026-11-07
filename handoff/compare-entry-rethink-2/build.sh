#!/bin/bash
# COMPARE-ENTRY-RETHINK-2 (mei): build the harness against glibc (dynamic) and Liella next (static, role tree),
# check both (and next with -AVX2), count in-function instructions / branches / taken branches, pack timing.tgz.
set -euo pipefail
W=$HOME/work/COMPARE-ENTRY-RETHINK-2
ROLE=$HOME/w/s1
cd "$W"
gcc -O2 -Wl,-z,now -DVARIANT='"glibc"' -o h_glibc harness2.c proto2.S
LIELLA_PRODUCTS=$ROLE/target/liella "$ROLE"/scripts/liella-cc -O2 -DVARIANT='"liella"' -o h_liella harness2.c proto2.S
./h_glibc check | tail -2
./h_liella check | tail -2
GLIBC_TUNABLES=glibc.cpu.hwcaps=-AVX2 ./h_liella check | tail -1
CPU=$(( $(nproc) - 3 ))
for mode in native sse2; do
    if [ "$mode" = sse2 ]; then export GLIBC_TUNABLES=glibc.cpu.hwcaps=-AVX2 PROTO_AVX2=0; fi
    PC_EVENTS='instructions,branches,r20c4' taskset -c "$CPU" ./h_glibc count
    PC_EVENTS='instructions,branches,r20c4' taskset -c "$CPU" ./h_liella count
done
tar -czf timing.tgz h_glibc h_liella time2.sh analyze2.py
echo BUILD-DONE
