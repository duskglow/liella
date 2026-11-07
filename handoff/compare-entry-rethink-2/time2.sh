#!/bin/bash
# COMPARE-ENTRY-RETHINK-2 timing host run: correctness check, 7 interleaved process rounds (glibc-linked and
# Liella-linked binary; both carry the prototype), one counter pass each. Read with: python3 -I analyze2.py <log>
cd "$(dirname "$0")" || exit 1
./h_glibc check | tail -1
./h_liella check | tail -1
for r in 1 2 3 4 5 6 7; do ./h_glibc time; ./h_liella time; done
PC_EVENTS='instructions,cycles,branches' ./h_glibc count
PC_EVENTS='instructions,cycles,branches' ./h_liella count
echo TIMING-DONE
