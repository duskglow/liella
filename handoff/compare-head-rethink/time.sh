#!/bin/bash
# COMPARE-HEAD-RETHINK timing host run: 5 interleaved process rounds of the glibc- and Liella-linked harness
# (both carry the prototype, so proto-in-glibc vs proto-in-liella is the control), then one counter pass each.
# Usage: time.sh <ivb|n100> <mode>...   mode: native | sse2 (GLIBC_TUNABLES=glibc.cpu.hwcaps=-AVX2, PROTO_AVX2=0)
cd "$(dirname "$0")" || exit 1
case $1 in
    ivb) EV="instructions,cycles,branches;instructions,r20c4,r010e;instructions,r0879,r0479" ;;
    n100) EV="instructions,cycles,branches;instructions,rc0c4,brmiss" ;;
    *) echo "unknown preset $1"; exit 2 ;;
esac
shift
./h_glibc check | tail -1
./h_liella check | tail -1
for mode in "$@"; do
    if [ "$mode" = sse2 ]; then export GLIBC_TUNABLES=glibc.cpu.hwcaps=-AVX2 PROTO_AVX2=0; fi
    for r in 1 2 3 4 5; do ./h_glibc time; ./h_liella time; done
    PC_EVENTS=$EV ./h_glibc count
    PC_EVENTS=$EV ./h_liella count
done
