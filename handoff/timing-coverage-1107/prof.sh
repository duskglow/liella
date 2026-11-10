#!/bin/sh
# TIMING-COVERAGE-1107 profiling: Liella per-symbol samples + Liella/glibc aggregate counters for the six
# NONE groups of the 1106E slower list. glibc side: aggregate counters and the bench's own timing only.
set -u
. ~/.cargo/env
cd ~/w/s2 || exit 2
W=~/work/TIMING-COVERAGE-1107
mkdir -p "$W"
echo "host CPU: $(grep -m1 'model name' /proc/cpuinfo)"; git -C ~/w/s2 log -1 --format='tree head %h'
cargo xtask bench --compare --build-only --only cbench_malloc,cbench_hot,cbench_memref,cbench_locale,cbench_opt || exit 3
B=~/w/s2/target/liella/bench
EV=cycles:u,instructions:u,branch-misses:u,L1-icache-load-misses:u,page-faults,instructions:k

prof() { # group tunables bench rows
  g=$1; t=$2; b=$3; r=$4
  echo "=== GROUP $g bench=$b tunables=${t:-none} rows=$r"
  for side in liella glibc; do
    if [ $side = liella ]; then bin=$B/$b; else bin=$B/$b.host-dynamic; fi
    for rep in 1 2; do
      echo "--- $side rep $rep region counters (CBENCH_COUNTERS=1)"
      env GLIBC_TUNABLES="$t" CBENCH_ROWS="$r" CBENCH_COUNTERS=1 taskset -c 3 "$bin" 2>&1 | tail -n 24
    done
    echo "--- $side perf stat (whole process, aggregate)"
    env GLIBC_TUNABLES="$t" CBENCH_ROWS="$r" taskset -c 3 perf stat -x, -e $EV "$bin" 2>&1 >/dev/null | tail -n 8
  done
  echo "--- liella perf record by symbol"
  env GLIBC_TUNABLES="$t" CBENCH_ROWS="$r" taskset -c 3 perf record -q -e cycles:u -c 20011 -o "$W/$g.data" "$B/$b" >/dev/null 2>&1
  perf report -i "$W/$g.data" --stdio --sort sym 2>/dev/null | grep -v '^#' | grep -v '^$' | head -n 16
}

NA=glibc.cpu.hwcaps=-AVX2
prof malloc "$NA" cbench_malloc malloc_free_16,malloc_free_32,malloc_free_128,malloc_free_1024,realloc_null_24,realloc_within_usable
prof memcmp "$NA" cbench_memref memcmp_diff8_64,memcmp_diff8_127,__memcmpeq_head_128,bcmp_head_128,__memcmpeq_eq_128,memcmp_256
prof strncmp "$NA" cbench_hot strncmp_diff8
prof strscan "$NA" cbench_hot strchr_16_hit,strchr_64_miss,strchrnul_64_hit,memrchr_64_hit
prof memchr "$NA" cbench_memref memchr_16,memchr_64,rawmemchr_4k
prof wctype "" cbench_locale iswctype_en_us,iswalpha_l_en_us,iswalpha_utf8,towupper_l_en_us,nl_langinfo_l_en_us,nl_langinfo_en_us

echo "=== GROUP cold bench=cbench_opt CBENCH_ONLY=cold (31 empty + 31 cold children per side)"
for side in liella glibc; do
  if [ $side = liella ]; then bin=$B/cbench_opt; else bin=$B/cbench_opt.host-dynamic; fi
  for rep in 1 2; do
    echo "--- $side rep $rep cold rows (bench's own medians)"
    env CBENCH_ONLY=cold CBENCH_ROWS=cold taskset -c 3 "$bin" 2>&1 | grep -E 'cold_|first_|second_' | head -n 30
  done
  echo "--- $side perf stat (inherit: parent + all children)"
  env CBENCH_ONLY=cold CBENCH_ROWS=cold taskset -c 3 perf stat -x, -e $EV "$bin" 2>&1 >/dev/null | tail -n 8
done
echo "--- liella perf record by symbol (cold children, user and kernel)"
env CBENCH_ONLY=cold CBENCH_ROWS=cold taskset -c 3 perf record -q -e cycles -c 2003 -o "$W/cold.data" "$B/cbench_opt" >/dev/null 2>&1
perf report -i "$W/cold.data" --stdio --sort dso,sym 2>/dev/null | grep -v '^#' | grep -v '^$' | head -n 30
echo PROF-DONE
