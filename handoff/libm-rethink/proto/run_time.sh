#!/bin/bash
# LIBM-RETHINK-2 timing job, started on a booked timing host under ~/bin/timing: the two prototype timing binaries
# (built on the build host by build_time.sh) alternated 5 times, 7 rounds of 10^6 calls per variant and mode each.
# Usage: run_time.sh <dir holding t_liella and t_glibc>
set -u
D=${1:?dir}
grep -m1 "model name" /proc/cpuinfo
uptime
for i in 1 2 3 4 5; do
    "$D/t_liella" 1000000 7 "liella-$i" || exit 1
    "$D/t_glibc" 1000000 7 "glibc-$i" || exit 1
done
uptime
