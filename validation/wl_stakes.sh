#!/bin/bash
# LAB-ONLY: this script drives the project's own test hosts and is not needed to build or test Liella.
# One command for the real-shape stakes table (STAKES-3): observe shapes -> percall spec -> Liella vs glibc -> stakes.md.
#
#   export S=<scratchpad>; bash validation/wl_stakes.sh [--out DIR] [--role R] [--shapes TSV | --spec C]
#       [--top K] [--cover F] [--title T] [--nowait] [<worktree|rev>]          (default rev: origin/next)
#   bash validation/wl_stakes.sh --collect DIR        (after --nowait: fetch the percall log, write DIR/stakes.md)
#
# Steps (all keke work goes through the queue: wl_shapes.sh -> vm_run, wl_percall.sh -> vm_run):
#  1. shapes: validation/wl_shapes.sh --role R --fetch DIR (the 30 workloads on the glibc kit, LD_PRELOAD observer).
#     Shapes are a property of the programs on glibc, not of the Liella tree: --shapes TSV reuses an earlier
#     shapes.tsv and --spec C an earlier generated spec (same spec = comparable tables across trees).
#  2. spec: validation/wl_stakes.py spec DIR/shapes.tsv DIR/spec.c [--top K] [--cover F].
#  3. percall: WLP_TAIL=0 validation/wl_percall.sh --base none --spec DIR/spec.c <target> > DIR/percall.log.
#  4. report: validation/wl_stakes.py report DIR/spec.c DIR/percall.log DIR/stakes.md.
# --nowait: steps 1-2 run in the foreground (shapes take minutes), step 3 is started detached; the label is saved in
# DIR/percall.label and --collect DIR finishes steps 3-4 (waits on the job when it is still running).
# DIR defaults to ./stakes-<sha9>. A local, unpushed commit works (wl_percall.sh ships it with git archive).
# Columns of stakes.md: rank, symbol, calls, covered share, stake (M instr:u), scaled, Liella/glibc, top shapes.
# Counts are instructions:u (deterministic); never time. Clean room: glibc is measured only as a black box.
set -u
here=$(cd "$(dirname "$0")" && pwd)
out=
role=
shapes=
spec=
top=
cover=
title=
nowait=0
collect=
while [ $# -gt 0 ]; do
    case $1 in
        --out) out=$2; shift 2 ;;
        --role) role=$2; shift 2 ;;
        --shapes) shapes=$2; shift 2 ;;
        --spec) spec=$2; shift 2 ;;
        --top) top=$2; shift 2 ;;
        --cover) cover=$2; shift 2 ;;
        --title) title=$2; shift 2 ;;
        --nowait) nowait=1; shift ;;
        --collect) collect=$2; shift 2 ;;
        -h|--help) sed -n '2,/^set -u/p' "$0" | sed '$d'; exit 0 ;;
        -*) echo "wl_stakes: unknown flag $1 (see --help)" >&2; exit 2 ;;
        *) break ;;
    esac
done
[ -n "${S:-}" ] && [ -f "$S/tools/vm_run.sh" ] || { echo "wl_stakes: export S=<scratchpad> first" >&2; exit 2; }
py=$(command -v python3 || command -v python) || { echo "wl_stakes: no python" >&2; exit 2; }
# python3 on Windows can be the store stub: check that it runs.
"$py" -c 'import sys' 2>/dev/null || py=$(command -v python) || exit 2

report() {   # <dir>: step 4, then the summary line
    local d=$1 t=()
    [ -f "$d/title" ] && t=(--title "$(cat "$d/title")")
    "$py" "$here/wl_stakes.py" report "$d/spec.c" "$d/percall.log" "$d/stakes.md" "${t[@]}" || return 1
    grep -q '^wl_percall: exit 0' "$d/percall.log" || echo "wl_stakes: WARNING percall did not exit 0 ($(grep -m1 '^wl_percall: exit' "$d/percall.log"))"
    grep -c MISMATCH "$d/percall.log" | awk '$1 > 0 {print "wl_stakes: WARNING " $1 " MISMATCH line(s) in percall.log"}'
    echo "wl_stakes: $d/stakes.md ($(grep -c '^| [0-9]' "$d/stakes.md") symbols)"
}

if [ -n "$collect" ]; then
    [ -f "$collect/percall.label" ] || { echo "wl_stakes: no $collect/percall.label (start with --nowait)" >&2; exit 2; }
    label=$(cat "$collect/percall.label")
    while VM_WAIT_MAX_MIN=4 bash "$S/tools/vm_wait.sh" keke "$label" tail > /dev/null 2>&1; [ $? = 3 ]; do :; done
    . "$S/tools/vm_common.sh"
    $(vm_ssh keke) "cat ~/logs/$label.log; echo \"wl_percall: exit \$(cat ~/logs/$label.done 2>/dev/null || echo ?) (log keke:~/logs/$label.log)\"" \
        > "$collect/percall.log" || exit 1
    report "$collect"
    exit $?
fi

[ $# -le 1 ] || { sed -n '2,/^set -u/p' "$0" | sed '$d'; exit 2; }
target=${1:-origin/next}
if [ -d "$target" ]; then target=$(cd "$target" && pwd); sha=$(git -C "$target" rev-parse HEAD) || exit 2
else sha=$(git -C "$here/.." rev-parse "$target") || exit 2
fi
out=${out:-stakes-${sha:0:9}}
mkdir -p "$out" || exit 1
out=$(cd "$out" && pwd)
echo "${title:-WL-STAKES: real-shape stakes per symbol at ${sha:0:9} (30 wl_count workloads, glibc kit)}" > "$out/title"
echo "wl_stakes: target ${sha:0:12} -> $out"

if [ -n "$spec" ]; then
    [ -f "$spec" ] && [ -f "$spec.json" ] || { echo "wl_stakes: --spec needs $spec and $spec.json" >&2; exit 2; }
    [ "$(cd "$(dirname "$spec")" && pwd)/$(basename "$spec")" = "$out/spec.c" ] || { cp "$spec" "$out/spec.c" && cp "$spec.json" "$out/spec.c.json"; } || exit 1
else
    if [ -n "$shapes" ]; then
        [ -f "$shapes" ] || { echo "wl_stakes: no $shapes" >&2; exit 2; }
        [ "$shapes" -ef "$out/shapes.tsv" ] || cp "$shapes" "$out/shapes.tsv" || exit 1
    else
        bash "$here/wl_shapes.sh" ${role:+--role "$role"} --fetch "$out" || { echo "wl_stakes: shapes run failed (see $out/check.tsv)"; exit 1; }
    fi
    "$py" "$here/wl_stakes.py" spec "$out/shapes.tsv" "$out/spec.c" ${top:+--top "$top"} ${cover:+--cover "$cover"} || exit 1
fi

if [ $nowait = 1 ]; then
    o=$(cd "$here/.." && WLP_TAIL=0 bash "$here/wl_percall.sh" --base none --spec "$out/spec.c" --nowait "$target" 2>&1)
    label=$(echo "$o" | sed -n 's/^re-attach: .*vm_wait.sh keke \([^ ]*\) .*/\1/p' | tail -1)
    [ -n "$label" ] || { echo "$o" | tail -5; echo "wl_stakes: percall did not start" >&2; exit 1; }
    echo "$label" > "$out/percall.label"
    echo "wl_stakes: percall started as $label; finish with: bash $here/wl_stakes.sh --collect $out"
    exit 0
fi
(cd "$here/.." && WLP_TAIL=0 bash "$here/wl_percall.sh" --base none --spec "$out/spec.c" "$target" > "$out/percall.log" 2>&1)
report "$out"
