#!/bin/sh
# TIMING-COVERAGE-1106C profiling: Liella per-symbol samples + Liella/glibc aggregate counters
# for the four NONE groups of the 1106 reopen list. glibc side: aggregate counters only.
set -u
. ~/.cargo/env
cd ~/w/sh2 || exit 2
W=~/work/TIMING-COVERAGE-1106C
mkdir -p "$W"
cargo xtask bench --compare --build-only --only cbench_malloc,cbench_hot,cbench_locale,cbench_opt || exit 3
B=~/w/sh2/target/liella/bench
ls -l "$B"/cbench_malloc* "$B"/cbench_hot* | head -8
EV=cycles:u,instructions:u,branch-misses:u,resource_stalls.any:u,idq_uops_not_delivered.core:u,uops_issued.any:u,cycle_activity.stalls_ldm_pending:u

prof() { # group tunables bench rows
  g=$1; t=$2; b=$3; r=$4
  echo "=== GROUP $g bench=$b tunables=${t:-none} rows=$r"
  for side in liella glibc; do
    if [ $side = liella ]; then bin=$B/$b; else bin=$B/$b.host-dynamic; fi
    echo "--- $side region counters (CBENCH_COUNTERS=1)"
    env GLIBC_TUNABLES="$t" CBENCH_ROWS="$r" CBENCH_COUNTERS=1 taskset -c 3 "$bin" 2>&1 | tail -n 40
    echo "--- $side perf stat (whole process, aggregate)"
    env GLIBC_TUNABLES="$t" CBENCH_ROWS="$r" taskset -c 3 perf stat -x, -e $EV "$bin" 2>&1 >/dev/null | tail -n 10
  done
  echo "--- liella perf record by symbol"
  env GLIBC_TUNABLES="$t" CBENCH_ROWS="$r" taskset -c 3 perf record -q -e cycles:u -c 20011 -o "$W/$g.data" "$B/$b" >/dev/null 2>&1
  perf report -i "$W/$g.data" --stdio --sort sym 2>/dev/null | grep -v '^#' | grep -v '^$' | head -n 14
}

NA=glibc.cpu.hwcaps=-AVX2
prof malloc "$NA" cbench_malloc malloc_free_16,malloc_free_64,malloc_free_256,malloc_free_1024,realloc_null_24,realloc_within_usable
prof strscan "$NA" cbench_hot strchr_16_hit,strchr_64_hit,strchr_64_miss,strchrnul_16_hit,strchrnul_64_hit,strchrnul_64_miss,strncmp_eq16
prof strcmp "" cbench_opt strcmp_names
prof strcasecmp "" cbench_locale strncasecmp_c_64
prof localeentry "$NA" cbench_locale nl_langinfo_en_us,towupper_l_en_us
echo PROF-DONE
