#!/bin/bash
# LAB-ONLY: this script drives the project's own test hosts and is not needed to build or test Liella.
# keke side of validation/wl_shapes.sh (WL-STAKES): argument shapes of the mem*/str*/malloc calls each workload's
# programs make, observed on the glibc kit with the LD_PRELOAD wrapper validation/wl_shapes_stub.c.
#   wl_shapes_run.sh <outdir> [<workload>...]     (default: every workload of wl_workloads.sh)
# Env: WLS_KIT (~/wl/kit/glibc/bin), WLS_WORK (~/wl/work).
# Writes <outdir>/: res/<workload>/<program>.<pid> (raw dumps), shapes.tsv (workload, symbol, size_lo, aux_lo,
#   flag, calls, sum_size, sum_aux; see the stub's header), check.tsv (workload, stdout SAME|DIFF, stderr SAME|DIFF,
#   exit status plain/observed, processes dumped). Exit 1 when any workload's output differs.
set -u
here=$(cd "$(dirname "$0")" && pwd)
. "$here/wl_workloads.sh"
[ $# -ge 1 ] || { sed -n '2,/^set -u/p' "$0" | sed '$d'; exit 2; }
out=$1
shift
K=${WLS_KIT:-$HOME/wl/kit/glibc/bin}
W=${WLS_WORK:-$HOME/wl/work}
rm -rf "$out/res"
mkdir -p "$out/res" || exit 1
gcc -O2 -U_FORTIFY_SOURCE -shared -fPIC -fno-builtin -fno-tree-loop-distribute-patterns -Wall -o "$out/wls_stub.so" \
    "$here/wl_shapes_stub.c" -ldl -lpthread || { echo "wl_shapes: stub build failed"; exit 1; }
[ $# -gt 0 ] || set -- $(for d in "${WL_DEFS[@]}"; do echo "${d%%|*}"; done)
: > "$out/shapes.tsv"
: > "$out/check.tsv"
cd "$W" || exit 1
for w in "$@"; do
    cmd=
    for d in "${WL_DEFS[@]}"; do [ "${d%%|*}" = "$w" ] && cmd=${d#*|}; done
    [ -n "$cmd" ] || { echo "wl_shapes: no workload $w"; continue; }
    c=${cmd//@/$K}
    r=$out/res/$w
    mkdir -p "$r"
    m0=$( (eval "$c") 2> "$r.err0" | md5sum | cut -c1-16; echo "rc=${PIPESTATUS[0]}")
    m1=$( (export WLS_DIR=$r LD_PRELOAD=$out/wls_stub.so; eval "$c") 2> "$r.err1" | md5sum | cut -c1-16; echo "rc=${PIPESTATUS[0]}")
    o=SAME; [ "$(echo "$m0" | head -1)" = "$(echo "$m1" | head -1)" ] || o=DIFF
    e=SAME; cmp -s "$r.err0" "$r.err1" || e=DIFF
    procs=$(ls "$r" | wc -l)
    printf '%s\t%s\t%s\t%s/%s\t%s\n' "$w" "$o" "$e" "$(echo "$m0" | tail -1)" "$(echo "$m1" | tail -1)" "$procs" >> "$out/check.tsv"
    echo "== $w stdout=$o stderr=$e $(echo "$m0" | tail -1)/$(echo "$m1" | tail -1) processes=$procs"
    cat "$r"/* 2>/dev/null | awk -F'\t' -v w="$w" '{ k = w "\t" $1 "\t" $2 "\t" $3 "\t" $4
        c[k] += $5; s[k] += $6; a[k] += $7 } END { for (k in c) print k "\t" c[k] "\t" s[k] "\t" a[k] }' |
        sort -t "$(printf '\t')" -k2,2 -k6,6nr >> "$out/shapes.tsv"
done
bad=$(awk -F'\t' '{ split($4, rc, "/") } $2 != "SAME" || $3 != "SAME" || rc[1] != rc[2] {n++} END {print n + 0}' "$out/check.tsv")
echo "wl_shapes: $(wc -l < "$out/check.tsv") workloads, $bad with different output; shapes: $out/shapes.tsv ($(wc -l < "$out/shapes.tsv") rows)"
[ "$bad" = 0 ]
