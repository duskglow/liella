#!/bin/bash
# LAB-ONLY: this script drives the project's own test hosts and is not needed to build or test Liella.
# wl_count.sh, keke side: deterministic counters of named workloads for glibc / BASE / NEW, one table each.
#
#   wl_count_run.sh WORKLOAD,WORKLOAD,... BASE_SHA NEW_SHA
#
# Kits (~/wl/kit/<variant>/bin, with at least the programs the workloads use) are made by wl_prep.sh
# first. Per workload and variant: one warm-up, `perf stat -r 3` (instructions:u, instructions:k,
# page-faults of the shell and its children), one run through md5sum (output check), one
# `strace -c -f`, three runs, median (syscalls: the gating number EXCLUDES the scheduling-dependent calls, reported apart, see below). Page faults are the MEDIAN of FAULT_RUNS (default 5) single runs, with
# min..max printed (one NUMA outlier run made a false +18%); instructions are the mean (exact). When new and
# base faults differ by > FAULT_PCT (default 2) % both are re-measured fresh in the same job and the table says so. Rows are cached in ~/wl/count/<variant>/<workload>.tsv (a variant
# is a commit: kit, inputs and the host's glibc do not change); COUNT_FRESH=1 ignores the cache.
# FUTEX (WLCOUNT-FUTEX, 2026-10-21): futex wait/wake counts of threaded workloads (sort-C, sort-utf8) depend on
# scheduling and vary run to run with identical code (3027..3638 in one build). The "syscalls" row is therefore the
# count EXCLUDING futex calls (deterministic, the gating number); the futex calls are the separate advisory row
# "futex (advisory)" and are never a verdict. Single-threaded workloads have futex 0, so their syscalls are unchanged.
# MTMEM (WLCOUNT-MTMEM-2, 2026-10-21; the rule of wl_run.sh, WLRUN-FUTEX, from the same code in wl_syscls.sh): when the run
# created a thread (a clone/clone3 with CLONE_THREAD, found by a second, tiny `strace -f -e trace=clone,clone3` run), ALL its
# mmap/munmap/mremap/brk/mprotect/madvise calls are the separate advisory row "mtmem (advisory)" and not in "syscalls" (a
# worker sometimes gets its own allocator arena, and arena alignment trims depend on addresses the kernel returns while
# other threads map). Single-threaded workloads keep those calls in the gating number.
# Row: instr_u  instr_sd%  instr_k  page-faults(median)  syscalls(gating)  md5  faults_min  faults_max  futex  mtmem
# (a cached row with fewer than 10 fields predates the mtmem split and is re-measured.)
# WLC_ENV='NAME=VALUE ...': environment for the measured command in every variant (see wl_count.sh); the
# cache file name carries a hash of it. Stdout of the measured command always goes into a pipe (| cat), never
# straight to /dev/null (GNU grep exits at the first match on /dev/null); cache files of this method end .p.tsv.
set -u
WL=${WL:-$HOME/wl}
list=$1
base=$2
new=$3
. "$(dirname "$0")/wl_workloads.sh"
work=$WL/work
tmp=$WL/tmp.count.$$
mkdir -p "$tmp"
trap 'rm -rf "$tmp"' EXIT
[ -d "$work/tree" ] || { mkdir -p "$work/tree" && tar -xf "$work/tree.tar" -C "$work/tree"; }

. "$(dirname "$0")/wl_stats.sh"
nruns=${FAULT_RUNS:-5}
TIME_DEPENDENT="ls-lR ls-l-enUS"   # workloads whose output depends on the current date (see measure)

ev() { echo "$1" | grep ",$2," | tail -1 | cut -d, -f1; }

wenv=${WLC_ENV:-}
etag=
[ -n "$wenv" ] && etag=.e$(printf '%s' "$wenv" | md5sum | cut -c1-8)

measure() { # workload cmd variant [fresh] [noenv] -> row on stdout; fresh=1 ignores the cache, noenv=1 drops WLC_ENV
    local name=$1 cmd=$2 v=$3 fresh=${4:-0} c cache e=$wenv t=$etag
    [ "${5:-0}" = 1 ] && { e=; t=; }
    cache=$WL/count/$v/$name$t.p.tsv
    # Time-dependent output (ls -l prints HH:MM for files younger than ~6 months, the year for older ones, so the
    # md5 changes when a file crosses that boundary): a row cached on an earlier day has a stale md5 and would
    # report DIFF against freshly measured rows (LSLR-OUTPUT-DIFF). Such a row is used only on the day it was made.
    case " $TIME_DEPENDENT " in *" $name "*)
        [ -s "$cache" ] && [ "$(date -r "$cache" +%F)" != "$(date +%F)" ] && fresh=1 ;;
    esac
    if [ "${COUNT_FRESH:-0}" != 1 ] && [ "$fresh" != 1 ] && [ -s "$cache" ] \
        && [ "$(awk -F'\t' 'NR == 1 { print NF }' "$cache")" = 10 ]; then cat "$cache"; return; fi
    c=${cmd//@/$WL/kit/$v/bin}
    [ -n "$e" ] && c="env $e $c"
    (cd "$work" && sh -c "$c" 2>/dev/null | cat > /dev/null)
    local runs="" i out sum sys
    for ((i = 0; i < nruns; i++)); do
        (cd "$work" && perf stat -x, -o "$tmp/perf.txt" -e instructions:u,instructions:k,page-faults sh -c "$c" 2>/dev/null | cat > /dev/null)
        out=$(cat "$tmp/perf.txt")
        runs+="$(ev "$out" instructions:u) $(ev "$out" instructions:k) $(ev "$out" page-faults)"$'\n'
    done
    local st
    st=$(printf '%s' "$runs" | wl_stats)   # iu sd% ik pf_med pf_min pf_max
    sum=$(cd "$work" && sh -c "$c" 2>/dev/null | md5sum | cut -c1-12)
    local k files= mt=0
    (cd "$work" && strace -f -e trace=clone,clone3 -o "$tmp/clone.txt" sh -c "$c" 2>/dev/null | cat > /dev/null)
    wl_has_thread "$tmp/clone.txt" && mt=1
    for k in 1 2 3; do   # three strace runs, the median of each number (a threaded program's mmap/munmap count can move by a few)
        (cd "$work" && strace -c -f -o "$tmp/strace$k.txt" sh -c "$c" 2>/dev/null | cat > /dev/null)
        files="$files $tmp/strace$k.txt"
    done
    sys=$(wl_strace_median $mt $files)   # "GATING FUTEX MTMEM"
    local fx=${sys#* }
    mkdir -p "$WL/count/$v"
    echo "$st" | awk -v sys="${sys%% *}" -v fx="${fx% *}" -v mx="${fx#* }" -v sum="$sum" -v OFS='\t' '{ print $1, $2, $3, $4, sys, sum, $5, $6, fx, mx }' | tee "$cache"
}

IFS=, read -r -a want <<< "$list"
for w in "${want[@]}"; do
    found=0
    for d in "${WL_DEFS[@]}"; do
        name=${d%%|*}
        [ "$name" = "$w" ] || continue
        found=1
        cmd=${d#*|}
        rg=$(measure "$name" "$cmd" glibc)
        rb=$(measure "$name" "$cmd" "$base")
        rn=$(measure "$name" "$cmd" "$new")
        note=
        envck=
        if [ -n "$wenv" ]; then   # self-check: did the env reach glibc? (its instructions must move)
            r0=$(measure "$name" "$cmd" glibc 0 1)
            if [ "$(echo "$r0" | cut -f1)" = "$(echo "$rg" | cut -f1)" ]; then
                envck="WARNING: glibc column unchanged under WLC_ENV='$wenv' ($(echo "$rg" | cut -f1) instr:u both) - the env did not reach glibc"
            else
                envck="glibc instr:u $(echo "$rg" | cut -f1) under WLC_ENV='$wenv' vs $(echo "$r0" | cut -f1) without: changed (ok)"
            fi
        fi
        if wl_faults_differ "$(echo "$rn" | cut -f4)" "$(echo "$rb" | cut -f4)" "${FAULT_PCT:-2}"; then
            first="new $(echo "$rn" | cut -f4) [$(echo "$rn" | cut -f7)..$(echo "$rn" | cut -f8)], base $(echo "$rb" | cut -f4) [$(echo "$rb" | cut -f7)..$(echo "$rb" | cut -f8)]"
            rb=$(measure "$name" "$cmd" "$base" 1)
            rn=$(measure "$name" "$cmd" "$new" 1)
            note="faults differed by > 2% (first measurement: $first): new and base re-measured fresh in this job"
        fi
        printf '%s\n%s\n%s\n' "$rn" "$rb" "$rg" | awk -F'\t' -v name="$name" -v nsha="${new:0:9}" -v bsha="${base:0:9}" -v note="$note" -v envck="$envck" '
            function ratio(a, b) { return (b + 0 > 0) ? sprintf("%.4f", a / b) : "-" }
            { for (i = 1; i <= 10; i++) f[NR, i] = $i }
            END {
                printf "== %s   (new %s, base %s)\n", name, nsha, bsha
                printf "%-18s %16s %16s %16s %9s %9s\n", "counter", "new", "base", "glibc", "new/glibc", "new/base"
                split("instructions:u instructions:k page-faults syscalls-gating", lab, " ")
                idx[1] = 1; idx[2] = 3; idx[3] = 4; idx[4] = 5
                for (k = 1; k <= 4; k++) {
                    i = idx[k]
                    printf "%-18s %16s %16s %16s %9s %9s\n", lab[k], f[1, i], f[2, i], f[3, i], ratio(f[1, i], f[3, i]), ratio(f[1, i], f[2, i])
                }
                printf "%-18s %16s %16s %16s %9s %9s\n", "futex (advisory)", f[1, 9], f[2, 9], f[3, 9], ratio(f[1, 9], f[3, 9]), ratio(f[1, 9], f[2, 9])
                printf "%-18s %16s %16s %16s %9s %9s\n", "mtmem (advisory)", f[1, 10], f[2, 10], f[3, 10], ratio(f[1, 10], f[3, 10]), ratio(f[1, 10], f[2, 10])
                printf "instr:u sd%%        %16s %16s %16s\n", f[1, 2], f[2, 2], f[3, 2]
                printf "faults min..max     %16s %16s %16s   (table shows the median)\n", f[1, 7] ".." f[1, 8], f[2, 7] ".." f[2, 8], f[3, 7] ".." f[3, 8]
                if (note != "") printf "note               %s\n", note
                if (envck != "") printf "env check          %s\n", envck
                printf "output             new vs glibc: %s   new vs base: %s\n", (f[1, 6] == f[3, 6] ? "SAME" : "DIFF"), (f[1, 6] == f[2, 6] ? "SAME" : "DIFF")
            }'
    done
    [ $found = 1 ] || echo "wl_count_run: unknown workload $w"
done
