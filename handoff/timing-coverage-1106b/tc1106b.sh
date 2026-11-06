#!/bin/bash
# TIMING-COVERAGE-1106B profiling job (keke:~/w/n1 = next c101c4c6). Liella: counters + samples by symbol.
# glibc (host-dynamic, the compare's reference): aggregate counters and strace -c only.
. ~/.cargo/env
cd ~/w/n1 || exit 2
O=~/logs/tc1106b
mkdir -p $O
echo "== compare"
cargo xtask bench --compare --counters --runs 3 cbench_process cbench_patterns > $O/compare.out 2>&1
echo "compare rc=$?"
B=target/liella/bench
EV=instructions:u,instructions:k,cycles:u,cycles:k,page-faults,context-switches
for row in none fork_exit_wait vfork_exit_wait posix_spawn_true posix_spawn_true_heap256m; do
  for side in liella glibc; do
    exe=$B/cbench_process; [ $side = glibc ] && exe=$B/cbench_process.host-dynamic
    echo "== stat $row $side"
    CBENCH_ROWS=$row perf stat -x, -e $EV $exe 2>&1 >/dev/null | cut -d, -f1,3 | tr '\n' ' '; echo
  done
done
for row in fork_exit_wait posix_spawn_true_heap256m; do
  for side in liella glibc; do
    exe=$B/cbench_process; [ $side = glibc ] && exe=$B/cbench_process.host-dynamic
    echo "== strace $row $side"
    CBENCH_ROWS=$row timeout 300 strace -f -c -o $O/strace.$row.$side $exe >/dev/null 2>&1
    head -22 $O/strace.$row.$side
  done
done
echo "== record liella fork_exit_wait (cpu-clock, user+kernel)"
CBENCH_ROWS=fork_exit_wait perf record -q -e cpu-clock -F 20000 -o $O/fork.data $B/cbench_process >/dev/null 2>&1
perf report -i $O/fork.data --stdio --sort sym 2>/dev/null | grep -v '^#' | grep -v '^$' | head -30
echo "== record liella posix_spawn_true_heap256m"
CBENCH_ROWS=posix_spawn_true_heap256m perf record -q -e cpu-clock -F 20000 -o $O/spawn.data $B/cbench_process >/dev/null 2>&1
perf report -i $O/spawn.data --stdio --sort sym 2>/dev/null | grep -v '^#' | grep -v '^$' | head -25
for row in none regexec_backref regexec_class_nomatch_1k; do
  for side in liella glibc; do
    exe=$B/cbench_patterns; [ $side = glibc ] && exe=$B/cbench_patterns.host-dynamic
    echo "== stat $row $side"
    CBENCH_ROWS=$row perf stat -x, -e instructions:u,cycles:u,branch-misses:u,L1-dcache-load-misses:u $exe 2>&1 >/dev/null | cut -d, -f1,3 | tr '\n' ' '; echo
  done
  [ $row = none ] && continue
  echo "== record liella $row (instructions:u)"
  CBENCH_ROWS=$row perf record -q -e instructions:u -c 20000 -o $O/$row.data $B/cbench_patterns >/dev/null 2>&1
  perf report -i $O/$row.data --stdio --sort sym 2>/dev/null | grep -v '^#' | grep -v '^$' | head -20
  echo "== record liella $row (cycles:u)"
  CBENCH_ROWS=$row perf record -q -e cycles:u -c 20000 -o $O/$row.cyc.data $B/cbench_patterns >/dev/null 2>&1
  perf report -i $O/$row.cyc.data --stdio --sort sym 2>/dev/null | grep -v '^#' | grep -v '^$' | head -12
done
echo "== maps (VMA count at row none, static Liella vs dynamic glibc)"
for side in liella glibc; do
  exe=$B/cbench_process; [ $side = glibc ] && exe=$B/cbench_process.host-dynamic
  CBENCH_ROWS=none $exe >/dev/null & p=$!; sleep 0; cat /proc/$p/maps 2>/dev/null | wc -l; wait $p
done
echo done
