#!/bin/bash
# LAB-ONLY: this script drives the project's own test hosts and is not needed to build or test Liella.
# Plants for the futex / threaded-memory split of wl_count and wl_run (WLCOUNT-FUTEX, WLCOUNT-MTMEM-2). Every plant must make
# a test fail; prints CAUGHT / SURVIVED per plant and exits 1 when one survives. No file in the tree is mutated:
#  - function plants (WL_STATS_PLANT=<name>) are overrides inside wl_stats_test.sh;
#  - file plants work on a copy of validation/ in RAM (${WL_PLANT_TMP:-/dev/shm/claude}/WLCOUNT-MTMEM-2, removed at the end) and run
#    wl_run_test.sh, wl_stats_test.sh and wl_mtmem_test.sh (real strace of a threaded sort, 3 runs) on it.
# EQUIVALENT: none by design.
here=$(cd "$(dirname "$0")" && pwd)
bash "$here/wl_stats_test.sh" > /dev/null || { echo "baseline wl_stats_test FAILED"; exit 1; }
bash "$here/wl_run_test.sh" > /dev/null || { echo "baseline wl_run_test FAILED"; exit 1; }
bash "$here/wl_mtmem_test.sh" 3 > /dev/null || { echo "baseline wl_mtmem_test FAILED"; exit 1; }
rc=0
for p in futex offby1 nofutex first nomt allmt; do
    if WL_STATS_PLANT=$p bash "$here/wl_stats_test.sh" > /dev/null; then echo "SURVIVED $p"; rc=1; else echo "CAUGHT $p"; fi
done
top=${WL_PLANT_TMP:-/dev/shm/claude}/WLCOUNT-MTMEM-2   # RAM; on sumire WL_PLANT_TMP=/mnt/zram/claude
rm -rf "$top"; mkdir -p "$top" || exit 1
trap 'rm -rf "$top"' EXIT
fplant() { # name file perl-substitution   (the copy is fresh for every plant)
    local d=$top/$1
    rm -rf "$d"; mkdir -p "$d"; cp "$here"/*.sh "$d"/
    perl -0pi -e "$3" "$d/$2" || { echo "BAD-PLANT $1"; rc=1; return; }
    cmp -s "$d/$2" "$here/$2" && { echo "BAD-PLANT $1 (no change)"; rc=1; return; }
    if bash "$d/wl_run_test.sh" > /dev/null 2>&1 && bash "$d/wl_stats_test.sh" > /dev/null 2>&1 && bash "$d/wl_mtmem_test.sh" 3 > /dev/null 2>&1; then
        echo "SURVIVED $1"; rc=1
    else echo "CAUGHT $1"; fi
}
fplant mem_never_split   wl_syscls.sh 's/if \(mt\) mx \+= n; else mm \+= n;/mm += n;/'
fplant mem_always_split  wl_syscls.sh 's/if \(mt\) mx \+= n; else mm \+= n;/mx += n;/'
fplant total_keeps_mtmem wl_syscls.sh 's/total - fx - mx,/total - fx,/'
fplant total_keeps_futex wl_syscls.sh 's/total - fx - mx,/total - mx,/'
fplant futex_not_split   wl_syscls.sh 's/s == "futex"/s == "futexx"/'
fplant thread_test_wrong wl_syscls.sh 's/grep -q .CLONE_THREAD./grep -q CLONE_CHILD/'
fplant munmap_dropped    wl_syscls.sh 's/\|munmap\|mremap/|mremap/'
fplant write_dropped     wl_syscls.sh 's/\|write\|pread64/|pread64/'
fplant count_no_thread   wl_count_run.sh 's/wl_has_thread "\$tmp\/clone.txt" && mt=1/:/'
fplant count_mt_ignored  wl_count_run.sh 's/wl_strace_median \$mt \$files/wl_strace_median 0 \$files/'
fplant count_cache_old   wl_count_run.sh 's/= 10 \]; then cat/= 9 ]; then cat/'
fplant run_no_thread     wl_run.sh 's/wl_has_thread "\$tmp\/clone.txt" && mt=1/:/'
fplant run_mt_ignored    wl_run.sh 's/wl_classify "\$tmp\/strace.txt" "\$mt"/wl_classify "\$tmp\/strace.txt" 0/'
exit $rc
