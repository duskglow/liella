#!/bin/bash
# LAB-ONLY: this script drives the project's own test hosts and is not needed to build or test Liella.
# End-to-end check of the threaded-workload rule (WLCOUNT-MTMEM-2) with REAL strace runs of a threaded program (sort --parallel=4
# on this host's sort): the gating syscall count that wl_count_run.sh and wl_run.sh share (wl_syscls.sh) is the same in every run;
# one more deterministic call changes it. Skips (exit 0, says so) when strace or sort --parallel is unavailable.
#   bash validation/wl_mtmem_test.sh [RUNS=6]
set -u
here=$(cd "$(dirname "$0")" && pwd)
. "$here/wl_stats.sh"
runs=${1:-6}
command -v strace > /dev/null && sort --parallel=4 < /dev/null > /dev/null 2>&1 || { echo "wl_mtmem_test: SKIP (no strace or sort --parallel)"; exit 0; }
t=$(mktemp -d) && trap 'rm -rf "$t"' EXIT
seq 1 1500000 | awk '{ print ($1 * 7919) % 100003 " line " $1 }' > "$t/in.txt"
measure() { # name cmd -> "GATING FUTEX MTMEM" of one run, thread flag from a clone trace
    local mt=0
    strace -f -e trace=clone,clone3 -o "$t/clone.txt" sh -c "$2" > /dev/null 2>&1
    wl_has_thread "$t/clone.txt" && mt=1
    strace -c -f -o "$t/s.txt" sh -c "$2" > /dev/null 2>&1
    echo "$mt $(wl_strace_sys "$t/s.txt" "$mt")"
}
cmd="LC_ALL=C sort --parallel=4 -S 200M -T $t $t/in.txt > /dev/null"
plus="$cmd; cat /dev/null"
declare -A seen
nfx=0
for ((i = 0; i < runs; i++)); do
    r=$(measure sort "$cmd"); mt=${r%% *}; g=$(echo "$r" | cut -d' ' -f2); seen[$g]=1
    echo "run $i: threads=$mt gating=$g futex=$(echo "$r" | cut -d' ' -f3) mtmem=$(echo "$r" | cut -d' ' -f4)"
    [ "$mt" = 1 ] || { echo "FAIL: sort --parallel=4 not seen creating threads"; exit 1; }
done
if [ "${#seen[@]}" != 1 ]; then echo "FAIL: gating count differs between identical threaded runs: ${!seen[*]}"; exit 1; fi
base=${!seen[*]}
r=$(measure plus "$plus"); g=$(echo "$r" | cut -d' ' -f2)
echo "plus one deterministic call (a cat): gating=$g (was $base)"
[ "$g" -gt "$base" ] || { echo "FAIL: an added deterministic call was not seen"; exit 1; }
echo "wl_mtmem_test: ok (gating $base in $runs runs)"
