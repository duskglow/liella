#!/bin/bash
# COMPARE-HEAD-RETHINK: build the black-box harness against glibc (dynamic, -z now) and Liella (static,
# scripts/liella-cc), run the correctness check, then per-call counters in the AVX2 and forced-SSE2 modes.
# Usage: run_count.sh <liella role dir> [events]; PROTO_FLAGS adds compiler/assembler flags (both builds).
set -euo pipefail
W=$HOME/work/COMPARE-HEAD-RETHINK
ROLE=$1
EV=${2:-instructions,cycles,branches;instructions,r20c4,r010e;instructions,r0879,r0479}
PF=${PROTO_FLAGS:-}
cd "$W"
gcc -O2 $PF -Wl,-z,now -DVARIANT='"glibc"' -o h_glibc harness.c proto.S
LIELLA_PRODUCTS=$ROLE/target/liella "$ROLE"/scripts/liella-cc -O2 $PF -DVARIANT='"liella"' -o h_liella harness.c proto.S
./h_glibc check
./h_liella check
CPU=$(( $(nproc) - 3 ))
for mode in avx2 sse2; do
    if [ "$mode" = sse2 ]; then export GLIBC_TUNABLES=glibc.cpu.hwcaps=-AVX2 PROTO_AVX2=0; fi
    PC_EVENTS=$EV taskset -c "$CPU" ./h_glibc count
    PC_EVENTS=$EV taskset -c "$CPU" ./h_liella count
done
