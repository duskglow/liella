#!/bin/bash
# LAB-ONLY: this script drives the project's own test hosts and is not needed to build or test Liella.
# Argument shapes of the mem*/str*/malloc calls real programs make (WL-STAKES). Exact counts, never time.
#
#   export S=<scratchpad>; bash validation/wl_shapes.sh [--role R] [--fetch DIR] [--nowait] [<workload>...]
#
# Method: an LD_PRELOAD wrapper (validation/wl_shapes_stub.c, one typed pass-through wrapper per function of the
# family) records per call a size bucket, an aux bucket (first difference for compares, old size for realloc...)
# and a flag (equal / found / moved), run on the glibc kit (keke ~/wl/kit/glibc/bin) over the workloads of
# validation/wl_workloads.sh (default: all). Only calls from the programs and their other shared libraries are
# observed; nothing inside the C library. Each workload runs twice, plain and observed: stdout, stderr and exit
# status must match (check.tsv; the run fails otherwise). Same limits as wl_entries (_exit/signal: no dump).
# Runs detached on keke under the queue (size S) in ~/w/<role> (default l, synced from this worktree first) and
# writes keke:~/wl/shapes/<role>/ (see validation/wl_shapes_run.sh). --fetch DIR copies shapes.tsv and check.tsv
# to DIR afterwards. Next: validation/wl_stakes.py turns shapes.tsv into a wl_percall spec and a stakes table.
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
        -*) echo "wl_shapes: unknown flag $1 (see --help)" >&2; exit 2 ;;
        *) break ;;
    esac
done
[ -n "${S:-}" ] && [ -x "$S/tools/vm_run.sh" -o -f "$S/tools/vm_run.sh" ] || { echo "wl_shapes: export S=<scratchpad> first" >&2; exit 2; }
[[ $role =~ ^[a-z]$ ]] || { echo "wl_shapes: --role is one letter" >&2; exit 2; }
for w in "$@"; do [[ $w =~ ^[A-Za-z0-9_-]+$ ]] || { echo "wl_shapes: bad workload name $w" >&2; exit 2; }; done
tree=$(cd "$(dirname "$0")/.." && pwd)
label=wls-$role
cmd="bash ~/w/$role/validation/wl_shapes_run.sh ~/wl/shapes/$role $*"
if [ $nowait = 1 ]; then
    VM_NOWAIT=1 VM_RUN_SIZE=S bash "$S/tools/vm_run.sh" --sync --tree "$tree" keke "$label" "$cmd"
    exit $?
fi
VM_RUN_SIZE=S bash "$S/tools/vm_run.sh" --sync --tree "$tree" keke "$label" "$cmd"
rc=$?
if [ -n "$fetch" ]; then
    . "$S/tools/vm_common.sh"
    mkdir -p "$fetch"
    for f in shapes.tsv check.tsv; do
        $(vm_ssh keke) "cat ~/wl/shapes/$role/$f" > "$fetch/$f"
    done
    echo "wl_shapes: fetched to $fetch"
fi
exit $rc
