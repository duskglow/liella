#!/bin/bash
# LAB-ONLY: this script drives the project's own test hosts and is not needed to build or test Liella.
# keke side of validation/wl_entries.sh (WL-ENTRIES): exact counts of the calls each workload's programs make
# into the C library's exported functions, measured on the glibc kit.
#   wl_entries_run.sh <outdir> [<workload>...]     (default: every workload of wl_workloads.sh)
# Env: WLE_KIT (~/wl/kit/glibc/bin), WLE_WORK (~/wl/work).
# Writes <outdir>/: libc_funcs.tsv, imports.tsv (program, name, version), syms.tsv (counted set), skipped.tsv,
#   res/<workload>/<program>.<pid> (raw dumps), table.tsv (workload, program, kit|sys, symbol, calls),
#   check.tsv (workload, stdout SAME|DIFF, stderr SAME|DIFF, exit status plain/counted, processes dumped).
set -u
here=$(cd "$(dirname "$0")" && pwd)
. "$here/wl_workloads.sh"
[ $# -ge 1 ] || { sed -n '2,/^set -u/p' "$0" | sed '$d'; exit 2; }
out=$1
shift
K=${WLE_KIT:-$HOME/wl/kit/glibc/bin}
W=${WLE_WORK:-$HOME/wl/work}
B=$out/build
rm -rf "$out/res"
mkdir -p "$out/res" "$B" || exit 1
libc=$(ldd "$K/sort" | awk '$1 == "libc.so.6" {print $3}')
[ -r "$libc" ] || { echo "wl_entries: no libc.so.6 found for $K/sort"; exit 1; }

# 1. libc's exported functions (name, version), and every libc function the kit's programs import.
readelf -W --dyn-syms "$libc" | awk '($4 == "FUNC" || $4 == "IFUNC") && $7 != "UND" && $8 ~ /@/ {
    n = $8; sub(/@@?/, "\t", n); print n }' | sort -u > "$out/libc_funcs.tsv"
kitprogs=" "
for f in "$K"/*; do
    [ -f "$f" ] && [ "$(head -c 4 "$f" | tail -c 3)" = ELF ] && kitprogs="$kitprogs$(basename "$f") "
done
for p in $kitprogs; do
    f=$K/$p
    readelf -W --dyn-syms "$f" | awk -v p="$p" '$7 == "UND" && ($4 == "FUNC" || $4 == "IFUNC") && $8 ~ /@GLIBC_/ {
        n = $8; sub(/@@?/, "\t", n); print p "\t" n }'
done | sort -u > "$out/imports.tsv"
# 2. the counted set: imported AND exported by this libc with that version; a name imported under two versions,
#    GLIBC_PRIVATE and the stub's own helpers (dlsym, dlvsym, pthread_atfork) are skipped (skipped.tsv says why).
awk -F'\t' -v skipf="$out/skipped.tsv" '
    NR == FNR { lf[$1 "\t" $2] = 1; next }
    { k = $2 "\t" $3
      if ($3 == "GLIBC_PRIVATE" || $2 ~ /^(dlsym|dlvsym|pthread_atfork|__register_atfork)$/) { why[$2] = "stub helper or private"; next }
      if (!(k in lf)) { why[$2] = "not exported by libc.so.6 as " $3; next }
      if (($2 in v) && v[$2] != $3) { why[$2] = "imported as " v[$2] " and " $3; next }
      if (!($2 in v)) { v[$2] = $3; o[++n] = $2 } }
    END { for (i = 1; i <= n; i++) if (!(o[i] in why)) print o[i] "\t" v[o[i]]
          for (s in why) print s "\t" why[s] > skipf }' "$out/libc_funcs.tsv" "$out/imports.tsv" > "$out/syms.tsv"
n=$(wc -l < "$out/syms.tsv")
{
    echo "#define WLE_N $n"
    printf '#define WLE_SYMS'
    awk -F'\t' '{ printf " \\\n    X(%d, %s, \"%s\")", NR - 1, $1, $2 }' "$out/syms.tsv"
    echo
} > "$B/wle_syms.h"
gcc -O2 -shared -fPIC -fno-tree-loop-distribute-patterns -I"$B" -o "$B/wle_stub.so" "$here/wl_entries_stub.c" -ldl ||
    { echo "wl_entries: stub build failed"; exit 1; }
echo "wl_entries: $n libc functions counted ($(wc -l < "$out/imports.tsv") imports by $(echo $kitprogs | wc -w) kit programs; skipped: $(wc -l < "$out/skipped.tsv"))"

# 3. every workload twice: plain, then under the counter; stdout/stderr/exit status must match.
[ $# -gt 0 ] || set -- $(for d in "${WL_DEFS[@]}"; do echo "${d%%|*}"; done)
: > "$out/table.tsv"
: > "$out/check.tsv"
cd "$W" || exit 1
for w in "$@"; do
    cmd=
    for d in "${WL_DEFS[@]}"; do [ "${d%%|*}" = "$w" ] && cmd=${d#*|}; done
    [ -n "$cmd" ] || { echo "wl_entries: no workload $w"; continue; }
    c=${cmd//@/$K}
    r=$out/res/$w
    mkdir -p "$r"
    m0=$( (eval "$c") 2> "$r.err0" | md5sum | cut -c1-16; echo "rc=${PIPESTATUS[0]}")
    m1=$( (export WLE_DIR=$r LD_PRELOAD=$B/wle_stub.so; eval "$c") 2> "$r.err1" | md5sum | cut -c1-16; echo "rc=${PIPESTATUS[0]}")
    o=SAME; [ "$(echo "$m0" | head -1)" = "$(echo "$m1" | head -1)" ] || o=DIFF
    e=SAME; cmp -s "$r.err0" "$r.err1" || e=DIFF
    procs=$(ls "$r" | wc -l)
    printf '%s\t%s\t%s\t%s/%s\t%s\n' "$w" "$o" "$e" "$(echo "$m0" | tail -1)" "$(echo "$m1" | tail -1)" "$procs" >> "$out/check.tsv"
    echo "== $w stdout=$o stderr=$e $(echo "$m0" | tail -1)/$(echo "$m1" | tail -1) processes=$procs"
    for f in "$r"/*; do
        [ -f "$f" ] || continue
        p=$(basename "$f"); p=${p%.*}
        case $kitprogs in *" $p "*) k=kit ;; *) k=sys ;; esac
        awk -v w="$w" -v p="$p" -v k="$k" -F'\t' '{ print w "\t" p "\t" k "\t" $1 "\t" $2 }' "$f"
    done | awk -F'\t' '{ s[$1 "\t" $2 "\t" $3 "\t" $4] += $5 } END { for (x in s) print x "\t" s[x] }' |
        sort -t "$(printf '\t')" -k1,1 -k5,5nr >> "$out/table.tsv"
done
bad=$(awk -F'\t' '{ split($4, rc, "/") } $2 != "SAME" || $3 != "SAME" || rc[1] != rc[2] {n++} END {print n + 0}' "$out/check.tsv")
echo "wl_entries: $(wc -l < "$out/check.tsv") workloads, $bad with different output; table: $out/table.tsv ($(wc -l < "$out/table.tsv") rows)"
[ "$bad" = 0 ]
