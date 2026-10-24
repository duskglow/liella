#!/bin/bash
# LAB-ONLY: this script drives the project's own test hosts and is not needed to build or test Liella.
# Which libc entry points real programs call, and how often (WL-ENTRIES). Exact dynamic call counts, never time.
#
#   export S=<scratchpad>; bash validation/wl_entries.sh [--role R] [--fetch DIR] [--nowait] [<workload>...]
#
# Method: an LD_PRELOAD stub (validation/wl_entries_stub.c) with one pass-through trampoline
# (`lock incq counter; jmp *real`) for EVERY libc function the glibc kit's programs import (taken from their
# dynamic symbol tables with readelf, not a hand list), run on the glibc kit (keke ~/wl/kit/glibc/bin) over the
# workloads of validation/wl_workloads.sh (default: all). Only calls from the programs and their other shared
# libraries bind to the trampolines; nothing inside the C library is observed. Each workload runs twice, plain and
# counted: stdout, stderr and exit status must match (check.tsv; the run fails otherwise).
# Limits: a process that ends with _exit or a signal writes no counts (fork children that _exit, e.g.); a name a
# program imports under two symbol versions is skipped (skipped.tsv); a vfork child's calls count for its parent.
# Runs detached on keke under the queue (size S) in ~/w/<role> (default l, synced from this worktree first) and
# writes keke:~/wl/entries/<role>/ (see validation/wl_entries_run.sh). --fetch DIR copies table.tsv, check.tsv,
# syms.tsv and skipped.tsv to DIR afterwards.
set -u
role=l
fetch=
nowait=0
while [ $# -gt 0 ]; do
    case $1 in
        --role) role=$2; shift 2 ;;
        --fetch) fetch=$2; shift 2 ;;
        --nowait) nowait=1; shift ;;
        -h|--help) sed -n '2,/^set -u/p' "$0" | sed '$d'; exit 0 ;;
        -*) echo "wl_entries: unknown flag $1 (see --help)" >&2; exit 2 ;;
        *) break ;;
    esac
done
[ -n "${S:-}" ] && [ -x "$S/tools/vm_run.sh" -o -f "$S/tools/vm_run.sh" ] || { echo "wl_entries: export S=<scratchpad> first" >&2; exit 2; }
[[ $role =~ ^[a-z]$ ]] || { echo "wl_entries: --role is one letter" >&2; exit 2; }
for w in "$@"; do [[ $w =~ ^[A-Za-z0-9_-]+$ ]] || { echo "wl_entries: bad workload name $w" >&2; exit 2; }; done
tree=$(cd "$(dirname "$0")/.." && pwd)
label=wle-$role
cmd="bash ~/w/$role/validation/wl_entries_run.sh ~/wl/entries/$role $*"
if [ $nowait = 1 ]; then
    VM_NOWAIT=1 VM_RUN_SIZE=S bash "$S/tools/vm_run.sh" --sync --tree "$tree" keke "$label" "$cmd"
    exit $?
fi
VM_RUN_SIZE=S bash "$S/tools/vm_run.sh" --sync --tree "$tree" keke "$label" "$cmd"
rc=$?
if [ -n "$fetch" ]; then
    . "$S/tools/vm_common.sh"
    mkdir -p "$fetch"
    for f in table.tsv check.tsv syms.tsv skipped.tsv; do
        $(vm_ssh keke) "cat ~/wl/entries/$role/$f" > "$fetch/$f"
    done
    echo "wl_entries: fetched to $fetch"
fi
exit $rc
