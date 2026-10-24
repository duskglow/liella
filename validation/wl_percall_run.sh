#!/bin/bash
# LAB-ONLY: this script drives the project's own test hosts and is not needed to build or test Liella.
# wl_percall.sh, keke side: instructions:u per call of probe cases for glibc / [BASE] / NEW.
#
#   wl_percall_run.sh SPEC_NAME CASES|all BASE_SHA|- NEW_SHA [TRACE]
#
# SPEC_NAME: a directory ~/wl/percall/spec/<name>/ holding percall.h, harness.c and spec.c (shipped by
# wl_percall.sh). Per variant (glibc, a commit) and spec content hash: one binary. glibc is linked DYNAMICALLY
# (gcc -O2 -Wl,-z,now: the system's libc.so.6, as every real program uses it; TOOLS-PERCALL 2026-10-10: static
# glibc ignores the locale for collation, so its locale rows were not a reference). -z now binds every PLT slot at
# startup, so the n = 0 run pays the whole ld.so cost and the per-call delta holds none of it. Liella: static,
# scripts/liella-cc against ~/wl/products/<sha>. Per case: `perf stat -r 5` of n=0 (setup only) and n=N; per call =
# (I(N) - I(0)) / (N * per). Rows cached in ~/wl/percall/<variant>/<hash>.<case>.tsv (glibc: variant glibc-dyn).
# Row: per_call  sd_per_call  calls  per
# EVENT: WL_EVENT (default instructions:u) is the counted perf event; rows of other events are cached as
# <hash>.<case>.<event>.tsv (instructions:u keeps the old name, so its cache stays valid).
# SELF-CHECK: each case is also run as `pc <case> check <dir>` in every variant; the column `check` is ok when all
# variants print the same line, MISMATCH when not (the glibc number is then no reference), - when the case has no check.
# Locale (PERCALL-LOCALE): every run has LC_ALL=$PC_LOCALE (default en_US.UTF-8) and the harness calls
# setlocale(LC_ALL, "") before each case (CASE_LOCALE cases: their own locale). LOCALE SELF-TEST: before measuring,
# each variant's `pc --locale` (setlocale result + nl_langinfo(CODESET)) must name the requested locale and its
# codeset and agree across variants, else exit 5. Cached rows are keyed by spec hash, locale and case.
# TRACE (wl_trace.sh): FUNC:SKIP:MAX - instead of the table, single-step the (SKIP+1)-th call of FUNC in NEW's
# binary for the first case under gdb (wl_trace.py) and print the instructions per function. Liella only, never glibc.
set -u
W=${WL:-$HOME/wl}
. "$(dirname "$0")/wl_lock.sh"
spec=$1; want=$2; base=$3; new=$4; trace=${5:-}
export LC_ALL=${PC_LOCALE:-en_US.UTF-8}
ev=${WL_EVENT:-instructions:u}
evtag=; [ "$ev" = instructions:u ] || evtag=.$(printf %s "$ev" | tr -c 'A-Za-z0-9_
-' _)
# ENVIRONMENT KEY (TOOL-PERCALL-TUNABLES-KEY): every environment input that reaches the measured process and can change a
# count is part of the row cache key: GLIBC_TUNABLES (dispatch level, malloc tunables), the loader/preload variables, the
# legacy MALLOC_* variables, LANGUAGE/LANG/LC_* (LC_ALL is keyed separately by file name), and any LIELLA_* variable.
# Unset and empty are the same; with none set the key is empty and the cache file names are unchanged.
envkey=; envdesc=
for n in GLIBC_TUNABLES LD_PRELOAD LD_LIBRARY_PATH LD_BIND_NOW LD_BIND_NOT LD_AUDIT LD_DEBUG LD_HWCAP_MASK LANGUAGE LANG \
         $(compgen -e | grep -E '^(LC_[A-Z]+|MALLOC_[A-Z_0-9]*|LIELLA_[A-Z_0-9]*)$' | grep -vx LC_ALL | sort); do
    [ -n "${!n:-}" ] && envdesc+="$n=${!n} "
done
[ -n "$envdesc" ] && envkey=.env$(printf %s "$envdesc" | md5sum | cut -c1-8)
sd=$W/percall/spec/$spec
h=$(cat "$sd"/*.h "$sd"/*.c | md5sum | cut -c1-10)
src=$W/percall/spec-$spec-$h
rm -rf "$src.tmp.$$"; cp -r "$sd" "$src.tmp.$$" && { mv -T "$src.tmp.$$" "$src" 2>/dev/null || rm -rf "$src.tmp.$$"; }
data=$W/percall/data
if [ ! -d "$data" ]; then
    mkdir -p "$data/d" && for i in $(seq 1 38); do : > "$data/d/f$i"; done
    : > "$data/d/a"; awk 'BEGIN { for (i = 1; i <= 200; i++) printf "line %d of the probe text file, some words here\n", i }' > "$data/d/text"
fi
dir=$data/d
variants=(glibc); [ "$base" != - ] && variants+=("$base"); variants+=("$new")
[ -n "$trace" ] && variants=("$new")

vdir() { [ "$1" = glibc ] && echo "$W/percall/glibc-dyn" || echo "$W/percall/$1"; }
build() { # variant -> binary path on stdout (built under a temporary name, then renamed: concurrent runs never see half a binary)
    local v=$1 out
    out=$(vdir "$1")/$h
    if [ ! -x "$out/pc" ]; then
        mkdir -p "$out"
        if [ "$v" = glibc ]; then
            # -lm: math specs (hypotf.c); gcc's default --as-needed leaves libm out of every other spec's binary.
            gcc -O2 -Wl,-z,now -I"$src" -o "$out/pc.$$" "$src"/harness.c "$src"/spec.c -lm || exit 1
        else
            [ -f "$W/products/$v/libc.a" ] || { echo "wl_percall: no products for $v" >&2; exit 1; }
            # The keke layout has ~/liella-test; other hosts (kanon) use the cached tree's own wrapper.
            lcc=$HOME/liella-test/scripts/liella-cc
            [ -x "$lcc" ] || lcc=$W/src/$v/scripts/liella-cc
            # -lm as for glibc: liella-cc links Liella's libm.a (math specs) only when -lm is given.
            LIELLA_PRODUCTS=$W/products/$v "$lcc" -O2 -I"$src" -o "$out/pc.$$" "$src"/harness.c "$src"/spec.c -lm || exit 1
        fi
        mv -f "$out/pc.$$" "$out/pc"
    fi
    echo "$out/pc"
}
for v in "${variants[@]}"; do
    [ "$v" = glibc ] || [ -f "$W/products/$v/libc.a" ] || { wl_lock "$v"; wl_build_products "$v" || exit 1; wl_unlock; }
done
declare -A bin
for v in "${variants[@]}"; do bin[$v]=$(build "$v") || exit 1; done
norm() { echo "$1" | tr 'A-Z' 'a-z' | tr -d '_-'; }   # UTF-8 = utf8
want_cs=; case $LC_ALL in *.*) want_cs=${LC_ALL#*.}; want_cs=${want_cs%%@*} ;; esac
loc_hdr=; loc_ref=; loc_bad=
for v in "${variants[@]}"; do
    lo=$(${bin[$v]} --locale 2>&1); lrc=$?
    loc_hdr+="${loc_hdr:+, }${v:0:9} [$lo]"
    read -r lname lcs <<< "$lo"
    if [ $lrc != 0 ] || [ "$lname" != "$LC_ALL" ]; then loc_bad+=" ${v:0:9}: locale [$lo] rc $lrc, requested $LC_ALL;"
    elif [ -n "$want_cs" ] && [ "$(norm "$lcs")" != "$(norm "$want_cs")" ]; then loc_bad+=" ${v:0:9}: codeset $lcs, requested $want_cs;"
    elif [ -n "$loc_ref" ] && [ "$lo" != "$loc_ref" ]; then loc_bad+=" ${v:0:9}: [$lo] differs from glibc [$loc_ref];"
    fi
    [ -z "$loc_ref" ] && loc_ref=$lo
done
echo "env key: GLIBC_TUNABLES=${GLIBC_TUNABLES:-<unset>}; row cache key ${envkey:-<none>}; keyed environment: ${envdesc:-<none set>}"
echo "locale: LC_ALL=$LC_ALL; setlocale(LC_ALL, \"\") + CODESET: $loc_hdr"
[ -z "$loc_bad" ] || { echo "LOCALE SELF-TEST FAILED:$loc_bad" >&2; exit 5; }
list=$("${bin[$new]}" --list) && [ -n "$list" ] || { echo "wl_percall: pc --list failed or lists no case" >&2; exit 1; }
[ "$want" = all ] && want=$(echo "$list" | awk '{printf "%s%s", s, $1; s=","}')
IFS=, read -r -a cases <<< "$want"

if [ -n "$trace" ]; then
    IFS=: read -r func skip max <<< "$trace"
    c=${cases[0]}
    echo "$list" | grep -q "^$c " || { echo "wl_trace: unknown case $c (known: $(echo "$list" | cut -d' ' -f1 | tr '\n' ' '))" >&2; exit 2; }
    echo "wl_trace: case $c, call $((skip + 1)) of $func, Liella ${new:0:9} (${bin[$new]})"
    WL_TRACE_FUNC=$func WL_TRACE_SKIP=$skip WL_TRACE_MAX=$max timeout 1800 \
        gdb -batch -nx -x "$(dirname "$0")/wl_trace.py" --args "${bin[$new]}" "$c" $((skip + 10)) "$dir" 2>&1 |
        grep -v -E '^0x[0-9a-f]+ in |^\[Inferior |^Breakpoint [0-9]+, '   # gdb's per-step "address in function ()" lines
    exit "${PIPESTATUS[0]}"
fi

ins() { # value and variance % of $ev; an event perf cannot count fails the run (never a silent 0)
    perf stat -r 5 -x, -e "$ev" "$@" 2>&1 >/dev/null | awk -F, -v e="$ev" '$3 == e || index($3, e ":") == 1 || index(e, $3) == 1 && $3 != "" {
        if ($1 !~ /^[0-9.]+$/) { print "wl_percall: perf cannot count " e " (" $1 ")" > "/dev/stderr"; exit 1 }
        print $1, $4 + 0; exit }'; }
measure() { # case variant -> row
    local c=$1 v=$2 cache calls per n a b
    cache=$(vdir "$2")/$h.$LC_ALL.$1$evtag$envkey.tsv
    if [ "${COUNT_FRESH:-0}" != 1 ] && [ -s "$cache" ]; then cat "$cache"; return; fi
    read -r _ calls per < <(echo "$list" | grep "^$c ")
    n=$(( 20000 / per )); [ $n -lt 100 ] && n=100
    ${bin[$v]} "$c" 0 "$dir" || { echo "wl_percall: case $c exits $? in ${v:0:9} (not measured)" >&2; return 1; }
    a=$(ins ${bin[$v]} "$c" 0 "$dir"); b=$(ins ${bin[$v]} "$c" $n "$dir")
    echo "$a $b" | awk -v n="$n" -v per="$per" -v calls="$calls" '{
        d = n * per; sa = $2 / 100 * $1; sb = $4 / 100 * $3
        printf "%.2f\t%.2f\t%s\t%s\n", ($3 - $1) / d, sqrt(sa * sa + sb * sb) / d, calls, per }' > "$cache.$$" && mv -f "$cache.$$" "$cache" && cat "$cache"   # whole row or none: no half file for a concurrent run
}
check() { # case -> ok | MISMATCH | - (self-check lines of every variant compared; mismatches listed after the table)
    local c=$1 v ref out st=ok
    ref=$(${bin[glibc]} "$c" check "$dir" 2>&1)
    for v in "${variants[@]}"; do
        [ "$v" = glibc ] && continue
        out=$(${bin[$v]} "$c" check "$dir" 2>&1)
        [ "$out" = "$ref" ] || { st=MISMATCH; echo "check $c: glibc [$ref] ${v:0:9} [$out]" >> "$mism"; }
    done
    [ -z "$ref" ] && [ $st = ok ] && st=-
    echo $st
}
v0=$(perf stat -x, -e "$ev" true 2>&1 >/dev/null | awk -F, 'NF > 2 { print $1; exit }')
case "$v0" in ""|*[!0-9.]*) echo "wl_percall: perf cannot count the event $ev here (perf says: ${v0:-nothing})" >&2; exit 2 ;; esac
tmp=$(mktemp); mism=$(mktemp)
{
for c in "${cases[@]}"; do
    echo "$list" | grep -q "^$c " || { echo "wl_percall: unknown case $c" >&2; continue; }
    rg=$(measure "$c" glibc) && rn=$(measure "$c" "$new") || { touch "$mism.fail"; continue; }
    rb=-; if [ "$base" != - ]; then rb=$(measure "$c" "$base") || { touch "$mism.fail"; continue; }; fi
    printf '%s\t%s\t%s\t%s\t%s\n' "$c" "$rg" "$rn" "$rb" "$(check "$c")"
done
} | awk -F'\t' -v hb=$([ "$base" != - ] && echo 1 || echo 0) '{
        g = $2; l = $6; calls = $4 + 0; w = calls * (l - g); ck = (hb ? $14 : $11)
        printf "%.0f\t%s\t%.1f\t%s\t%.1f\t%.2f\t%.2f\t%.2f\t%d\t%s\n", w, $1, g, (hb ? sprintf("%.1f", $10) : "-"), l, (g > 0 ? l / g : 0), $7, $3, calls, ck }' |
    sort -t$'\t' -k1,1 -g -r > "$tmp"
printf '%s per call%s (new %s%s; glibc dynamic, LC_ALL=%s); sorted by (new-glibc) x typical calls\n' "$ev" "$([ "$ev" = instructions:u ] && echo ", deterministic" || echo ", NOT deterministic: read the sd columns")" "${new:0:9}" "$([ "$base" != - ] && echo ", base ${base:0:9}")" "$LC_ALL"
printf '%-18s %9s %9s %9s %9s %8s %9s %14s  %s\n' case glibc base new new/glibc sd_new sd_glibc "(new-glibc)*calls" check
awk -F'\t' '{ printf "%-18s %9.1f %9s %9.1f %9.2f %8.2f %9.2f %14s  %s\n", $2, $3, $4, $5, $6, $7, $8, ($9 > 0 ? $1 : "-"), $10 }' "$tmp"
[ -s "$mism" ] && { echo "SELF-CHECK MISMATCH (the glibc column of these rows is not a reference):"; cat "$mism"; }
st=0; [ -e "$mism.fail" ] && { echo "wl_percall: some cases failed and are missing from the table (see above)"; st=1; }
rm -f "$tmp" "$mism" "$mism.fail"
exit $st
