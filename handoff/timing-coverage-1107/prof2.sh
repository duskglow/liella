#!/bin/sh
# TIMING-COVERAGE-1107 strstr_64 group: region counters both sides, Liella symbols. Uses prof.sh's bench build.
set -u
B=~/w/s2/target/liella/bench; W=~/work/TIMING-COVERAGE-1107
R=strstr_hit_64,strstr_miss_64,strstr_hit_4k
for t in "" glibc.cpu.hwcaps=-AVX2; do
  for side in liella glibc; do
    if [ $side = liella ]; then bin=$B/cbench_memref; else bin=$B/cbench_memref.host-dynamic; fi
    for rep in 1 2; do
      echo "--- $side rep $rep tunables=${t:-none}"
      env GLIBC_TUNABLES="$t" CBENCH_ROWS=$R CBENCH_COUNTERS=1 taskset -c 3 "$bin" 2>&1 | grep '^ctr'
    done
  done
  echo "--- liella perf record tunables=${t:-none}"
  env GLIBC_TUNABLES="$t" CBENCH_ROWS=$R taskset -c 3 perf record -q -e cycles:u -c 2003 -o "$W/strstr.data" "$B/cbench_memref" >/dev/null 2>&1
  perf report -i "$W/strstr.data" --stdio --sort sym 2>/dev/null | grep -v '^#' | grep -v '^$' | head -n 6
done
echo PROF2-DONE
