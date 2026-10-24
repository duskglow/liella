#!/bin/bash
# LAB-ONLY: this script drives the project's own test hosts and is not needed to build or test Liella.
# Workload gate, attribution (runs on a timing host): for the workloads that failed a counter,
# the top Liella symbols by user instructions, NEW against BASE.
#
#   wl_attr.sh BASE NEW WORKLOAD...        (WL as in wl_run.sh)
#
# perf record -e instructions:u -c 1000000 (one sample per million user instructions) on the
# BASE and NEW kits only, never on glibc (clean room: no glibc symbols are sampled or shown).
# Only samples whose dso is a program of the kit are kept (the sh wrapper and the loader are
# dropped). Prints the 5 symbols whose instruction count grew most and the 5 largest in NEW.
set -u
PATH=$HOME/.cargo/bin:$HOME/bin:$PATH
WL=${WL:-$HOME/wl}
. "$(dirname "$0")/wl_workloads.sh"
base=$1 new=$2
shift 2
tmp=$WL/tmp.attr.$$
mkdir -p "$tmp"
trap 'rm -rf "$tmp"' EXIT
progs=$(ls "$WL/kit/$new/bin" | paste -sd'|')
for want in "$@"; do
    for d in "${WL_DEFS[@]}"; do
        name=${d%%|*}
        [ "$name" = "$want" ] || continue
        cmd=${d#*|}
        for v in $base $new; do
            c=${cmd//@/$WL/kit/$v/bin}
            (cd "$WL/work" && perf record -q -e instructions:u -c 1000000 -o "$tmp/$v.data" sh -c "$c" > /dev/null 2>&1)
            perf report -i "$tmp/$v.data" --stdio --no-children -n --sort dso,sym 2>/dev/null | grep -v '^#' | \
                awk -v re="^($progs)\$" '$3 ~ re { s = $5; for (i = 6; i <= NF; i++) s = s " " $i; print $2 "\t" s }' > "$tmp/$v.sym"
            # v0-mangled Rust symbols -> readable names when rustfilt is installed (~/.cargo/bin or ~/bin)
            if command -v rustfilt > /dev/null 2>&1; then
                cut -f2 "$tmp/$v.sym" | rustfilt > "$tmp/$v.dem"
                paste <(cut -f1 "$tmp/$v.sym") "$tmp/$v.dem" > "$tmp/$v.sym2" && mv "$tmp/$v.sym2" "$tmp/$v.sym"
            fi
        done
        echo "== $want: top Liella symbols by user instructions (Minstr = samples; base $base -> new $new)"
        awk -F'\t' 'NR == FNR { b[$2] += $1; next } { n[$2] += $1 } END {
            for (s in n) printf "%d\t%d\t%d\t%s\n", n[s] - b[s], b[s], n[s], substr(s, 1, 110)
            for (s in b) if (!(s in n)) printf "%d\t%d\t0\t%s\n", -b[s], b[s], substr(s, 1, 110) }' \
            "$tmp/$base.sym" "$tmp/$new.sym" > "$tmp/diff.tsv"
        echo "  grew most (delta, base, new in Minstr):"
        sort -t$'\t' -k1,1nr "$tmp/diff.tsv" | head -5 | sed 's/^/    /'
        echo "  largest in NEW:"
        sort -t$'\t' -k3,3nr "$tmp/diff.tsv" | head -5 | sed 's/^/    /'
    done
done
