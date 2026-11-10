#!/bin/bash
# LAB-ONLY: this script drives the project's own test hosts and is not needed to build or test Liella.
# Self-test of the syscall classification in validation/wl_syscls.sh, used by wl_run.sh and wl_count_run.sh (WLRUN-FUTEX): futex and, for a run that created
# threads, the memory-mapping calls are reported apart and are not in sys_total / sys_mem / sys_other.
#   bash validation/wl_run_test.sh                 -> exit 0 and "wl_run_test: N passed" or exit 1
#   WL_RUN_SCRIPT=<copy of wl_run.sh> WL_SYSCLS=<copy of wl_syscls.sh> bash ...   -> test (planted) copies instead (plants use this)
set -u
here=$(cd "$(dirname "$0")" && pwd)
script=${WL_RUN_SCRIPT:-$here/wl_run.sh}
syscls=${WL_SYSCLS:-$here/wl_syscls.sh}
. "$syscls"
classify=$WL_CLASSIFY
[ -n "${classify:-}" ] || { echo "wl_run_test: WL_CLASSIFY not found in $syscls"; exit 1; }
pass=0; fail=0
check() { # name expected actual
    if [ "$2" = "$3" ]; then pass=$((pass + 1)); else fail=$((fail + 1)); echo "FAIL $1: expected [$2] got [$3]"; fi
}
# strace -c table after the sed in wl_run.sh (leading blanks stripped, runs of blanks squeezed); some rows carry an errors column
table='% time seconds usecs/call calls errors syscall
------ ----------- ----------- --------- --------- ----------------
60.00 0.300000 3 100 futex
20.00 0.100000 1 200 write
10.00 0.050000 5 10 2 mmap
5.00 0.025000 1 6 munmap
3.00 0.015000 1 4 mprotect
2.00 0.010000 1 20 read
1.00 0.005000 1 3 clone3
1.00 0.005000 1 2 getpid
------ ----------- ----------- --------- --------- ----------------
100.00 0.510000 1 345 2 total'
# fields: total rw open fstat stat dir mem proc other futex mtmem piperw
single=$(echo "$table" | awk -v mt=0 "$classify")
threaded=$(echo "$table" | awk -v mt=1 "$classify")
t() { printf '%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s' "$@"; }
check "single-threaded run: futex apart, mem kept"  "$(t 245 220 0 0 0 0 20 3 2 100 0 0)" "$single"
check "threaded run: futex and mem apart"           "$(t 225 220 0 0 0 0 0 3 2 100 20 0)" "$threaded"
check "field count" 12 "$(echo "$single" | awk -F'\t' '{ print NF }')"
# the deterministic classes stay in the total in both cases
check "write is still counted (threaded)" 220 "$(echo "$threaded" | cut -f2)"
check "other without futex"               2   "$(echo "$single" | cut -f9)"
check "no futex row" "$(t 12 10 0 0 0 0 0 0 2 0 0 0)" "$(printf '%s\n' '% time seconds usecs/call calls errors syscall' '------ - - - - -' '90.00 0.9 1 10 write' '10.00 0.1 1 2 getpid' '------ - - - - -' '100.00 1.0 1 12 total' | awk -v mt=1 "$classify")"
# the script feeds the thread flag to the classifier (found by a clone/clone3 trace) - the awk test above cannot see that
check "wl_run passes mt to wl_classify" 1 "$(grep -c -- 'wl_classify "$tmp/strace.txt" "$mt"' "$script")"
check "wl_run detects CLONE_THREAD with wl_has_thread" 1 "$(grep -c 'wl_has_thread "$tmp/clone.txt" && mt=1' "$script")"
check "wl_run passes mp to wl_classify" 1 "$(grep -c -- 'wl_classify "$tmp/strace.txt" "$mt" "$mp"' "$script")"
check "wl_run detects a child process with wl_has_child" 1 "$(grep -c 'wl_has_child "$tmp/clone.txt" && mp=1' "$script")"
check "wl_run traces pipe fds" 1 "$(grep -c 'strace -C -f -y -o "$tmp/strace.txt"' "$script")"
check "wl_run clone trace sees forks" 1 "$(grep -c 'strace -f -e trace=clone,clone3,fork,vfork -o' "$script")"
check "wl_run row has sys_piperw" 1 "$(grep -c 'cut -f10-12' "$script")"
check "wl_run sources the shared classifier" 1 "$(grep -c 'wl_syscls.sh' "$script")"
# wl_classify (the function both tools call) and wl_has_thread, on files
tt=$(mktemp -d); trap 'rm -rf "$tt"' EXIT
echo "$table" > "$tt/s.txt"
check "wl_classify threaded"  "$(t 225 220 0 0 0 0 0 3 2 100 20 0)" "$(wl_classify "$tt/s.txt" 1)"
check "wl_classify single"    "$(t 245 220 0 0 0 0 20 3 2 100 0 0)" "$(wl_classify "$tt/s.txt" 0)"
printf '%s\n' '1234 clone3({flags=CLONE_VM|CLONE_FS|CLONE_THREAD|CLONE_SYSVSEM}, 88) = 1235' > "$tt/c1.txt"
printf '%s\n' '1234 clone(child_stack=NULL, flags=CLONE_CHILD_CLEARTID|CLONE_CHILD_SETTID|SIGCHLD) = 1235' > "$tt/c2.txt"
check "wl_has_thread yes" 0 "$(wl_has_thread "$tt/c1.txt"; echo $?)"
check "wl_has_thread no (fork)" 1 "$(wl_has_thread "$tt/c2.txt"; echo $?)"
check "wl_has_child no (thread only)" 1 "$(wl_has_child "$tt/c1.txt"; echo $?)"
check "wl_has_child yes (fork via clone)" 0 "$(wl_has_child "$tt/c2.txt"; echo $?)"
printf '%s\n' '1234 clone3({flags=CLONE_VM|CLONE_THREAD}, 88) = 1235' '1234 vfork() = 1236' > "$tt/c3.txt"
check "wl_has_child yes (vfork next to a thread)" 0 "$(wl_has_child "$tt/c3.txt"; echo $?)"
# D-X.46: strace -C -f -y output (trace lines, then the summary) of a pipeline; pipe calls leave rw and the total only for MP=1
ptab='100 pipe2([3<pipe:[9]>, 4<pipe:[9]>], 0) = 0
100 write(4<pipe:[9]>, "ab", 2) = 2
101 read(3<pipe:[9]>, "ab", 4096) = 2
101 read(3<pipe:[9]>, <unfinished ...>
100 read(5</etc/passwd>, "x", 1) = 1
101 <... read resumed>"", 4096) = 0
% time seconds usecs/call calls errors syscall
------ ----------- ----------- --------- --------- ----------------
80.00 0.1 1 3 read
10.00 0.1 1 1 write
10.00 0.1 1 1 pipe2
------ ----------- ----------- --------- --------- ----------------
100.00 0.3 1 5 total'
check "pipeline: pipe calls advisory"   "$(t 2 1 1 0 0 0 0 0 0 0 0 3)" "$(echo "$ptab" | awk -v mt=0 -v mp=1 "$classify")"
check "single process: pipe calls kept" "$(t 5 4 1 0 0 0 0 0 0 0 0 0)" "$(echo "$ptab" | awk -v mt=0 -v mp=0 "$classify")"
echo "$ptab" > "$tt/p.txt"
check "wl_classify passes MP" "$(t 2 1 1 0 0 0 0 0 0 0 0 3)" "$(wl_classify "$tt/p.txt" 0 1)"
echo "wl_run_test: $pass passed, $fail failed"
[ "$fail" = 0 ]
