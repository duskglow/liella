#!/bin/bash
# LAB-ONLY: this script drives the project's own test hosts and is not needed to build or test Liella.
# Self-test of validation/wl_stats.sh (median / min / max and the 2% re-measure trigger).
#   bash validation/wl_stats_test.sh          -> exit 0 and "wl_stats_test: N passed" or exit 1
#   WL_STATS_PLANT=mean bash validation/wl_stats_test.sh   -> plants a mean-based faults column; MUST fail
#   WL_STATS_PLANT=futex|offby1|nofutex|first|nomt|allmt bash validation/wl_stats_test.sh  -> plants of wl_strace_sys (the syscall split); each MUST fail (validation/wl_stats_plants.sh runs them all)
set -u
here=$(cd "$(dirname "$0")" && pwd)
. "$here/wl_stats.sh"
if [ "${WL_STATS_PLANT:-}" = mean ]; then
    # the old behaviour: mean of the faults instead of the median
    wl_stats() { awk '{ n++; a += $1; b += $2; c += $3; p[n] = $3 }
        END { printf "%.0f 0.00 %.0f %.0f %d %d\n", a / n, b / n, c / n, p[1], p[n] }'; }
fi
pass=0
fail=0
check() { # name expected actual
    if [ "$2" = "$3" ]; then pass=$((pass + 1)); else fail=$((fail + 1)); echo "FAIL $1: expected [$2] got [$3]"; fi
}
checkrc() { # name expected_rc command...
    local name=$1 want=$2
    shift 2
    "$@"
    check "$name" "$want" "$?"
}

# the gzip-c incident: one outlier (479) among stable 309-310 runs
out=$(printf '%s\n' "1000 5 309" "1000 5 479" "1000 5 310" "1000 5 309" "1000 5 310" | wl_stats)
check outlier_median "1000 0.00 5 310 309 479" "$out"
# the mean would have been 363.4 -> the median is what must be reported
check three_runs "1000 0.00 5 310 309 479" "$(printf '%s\n' "1000 5 479" "1000 5 310" "1000 5 309" | wl_stats)"
# even count: middle two averaged, rounded down
check even_count "1000 0.00 5 310 309 312" "$(printf '%s\n' "1000 5 312" "1000 5 309" "1000 5 311" "1000 5 310" | wl_stats)"
# single run, all equal
check single "7 0.00 1 42 42 42" "$(printf '%s\n' "7 1 42" | wl_stats)"
# instructions: mean and sd
check instr_sd "150 47.14 5 3 3 3" "$(printf '%s\n' "100 5 3" "200 5 3" | wl_stats)"
check empty "- - - - - -" "$(printf '' | wl_stats)"

checkrc differ_big 0 wl_faults_differ 479 310
checkrc differ_same 1 wl_faults_differ 310 310
checkrc differ_edge_2pct 1 wl_faults_differ 102 100
checkrc differ_over_2pct 0 wl_faults_differ 103 100
checkrc differ_below 0 wl_faults_differ 97 100
checkrc differ_base0 0 wl_faults_differ 5 0
checkrc differ_both0 1 wl_faults_differ 0 0

# wl_strace_sys / wl_strace_median (WLCOUNT-FUTEX, WLCOUNT-MTMEM-2): "GATING FUTEX MTMEM" from `strace -c -f` summaries, with and
# without the errors column; the classification is wl_syscls.sh's, the same code wl_run.sh uses.
case "${WL_STATS_PLANT:-}" in
    futex) wl_strace_sys() { sed 's/^ *//; s/  */ /g' "$1" | awk -v mt="${2:-0}" '$NF == "total" { print $4, 0, 0 }'; } ;;   # the old total incl. futex and mtmem
    offby1) wl_strace_sys() { local l; l=$(wl_classify "$1" "${2:-0}") || return; echo "$(echo "$l" | awk -F'\t' '{ print $1 - 1, $10, $11 }')"; } ;;   # the gating count one too low
    first) wl_strace_median() { shift; wl_strace_sys "$1" 1; } ;;   # one strace run instead of the median of three
    nomt) wl_strace_median() { local mt=$1; shift; mt=0; for f in "$@"; do wl_strace_sys "$f" 0; done | awk '{ n++; a[n] = $1; b[n] = $2; c[n] = $3 } END { print a[(n + 1) / 2], b[(n + 1) / 2], c[(n + 1) / 2] }'; } ;;   # thread flag ignored
    allmt) wl_strace_sys() { local l; l=$(wl_classify "$1" 1); echo "$l" | awk -F'\t' '{ print $1, $10, $11 }'; } ;;   # every run treated as threaded
    nofutex) WL_CLASSIFY=${WL_CLASSIFY//'s == "futex"'/'s == "nothing"'} ;;   # futex never recognised
esac
t=$(mktemp -d) && trap 'rm -rf "$t"' EXIT
printf '%s\n' '% time     seconds  usecs/call     calls    errors syscall' '------ ----------- ----------- --------- --------- ----------------' \
    ' 60.00    0.000100           1       300        12 futex' ' 30.00    0.000050           1       100           read' \
    ' 10.00    0.000010           1        50         3 openat' '  5.00    0.000010           1        20           fstat' \
    '  2.00    0.000010           1        30           mmap' '  1.00    0.000010           1         8           munmap' '------ ----------- ----------- --------- --------- ----------------' \
    '100.00    0.000160           1       508        15 total' > "$t/e.txt"
check strace_errors_col_single "208 300 0" "$(wl_strace_sys "$t/e.txt" 0)"        # mmap/munmap stay in the gate: 100 + 50 + 20 + 38
check strace_errors_col_threaded "170 300 38" "$(wl_strace_sys "$t/e.txt" 1)"
check strace_default_single "208 300 0" "$(wl_strace_sys "$t/e.txt")"
printf '%s\n' '% time     seconds  usecs/call     calls syscall' '------ ----------- ----------- --------- ----------------' \
    ' 90.00    0.000100           1       300 futex' ' 10.00    0.000050           1       100 read' \
    '------ ----------- ----------- --------- ----------------' '100.00    0.000160           1       400 total' > "$t/n.txt"
check strace_no_errors_col "100 300 0" "$(wl_strace_sys "$t/n.txt" 1)"
printf '%s\n' '% time     seconds  usecs/call     calls    errors syscall' ' 50.00    0.0           1       100           read' '100.00    0.0           1       100 total' > "$t/z.txt"
check strace_no_futex "100 0 0" "$(wl_strace_sys "$t/z.txt" 1)"
# a threaded run: two runs that differ only in futex waits and mapping calls give the same gating number (the point of MTMEM-2)
sed 's/ 300        12 futex/ 777        12 futex/; s/ 30 *mmap/ 31           mmap/; s/ 8 *munmap/ 5           munmap/; s/ 508 .* total/ 983 15 total/' "$t/e.txt" > "$t/e2.txt"
check strace_threaded_two_runs_same_gating "$(wl_strace_sys "$t/e.txt" 1 | cut -d' ' -f1)" "$(wl_strace_sys "$t/e2.txt" 1 | cut -d' ' -f1)"
check strace_threaded_two_runs_differ_advisory "170 777 36" "$(wl_strace_sys "$t/e2.txt" 1)"
# one more deterministic call (a code change adding a syscall per call) must show, threaded or not: read 100 -> 101
sed 's/ 100           read/ 101           read/; s/ 508 .* total/ 509 15 total/' "$t/e.txt" > "$t/p.txt"
check strace_plus_one_threaded "171 300 38" "$(wl_strace_sys "$t/p.txt" 1)"
check strace_plus_one_single "209 300 0" "$(wl_strace_sys "$t/p.txt" 0)"
# median of three: gating 807 / 804 / 804 -> 804; futex 862 / 846 / 943 -> 862; mtmem 5 / 9 / 7 -> 7
for n in 1 2 3; do printf '%s\n' '% time seconds usecs/call calls syscall' '------ ------ ------ ------ ------' ' 60.00    0.0           1       X          futex' ' 40.00    0.0           1       Y           mmap' ' 10.00    0.0           1       W           write' '100.00    0.0           1       Z total' > "$t/m$n.txt"; done
sed -i 's/X/862/; s/Y/5/; s/W/807/; s/Z/1674/' "$t/m1.txt"; sed -i 's/X/846/; s/Y/9/; s/W/804/; s/Z/1659/' "$t/m2.txt"; sed -i 's/X/943/; s/Y/7/; s/W/804/; s/Z/1754/' "$t/m3.txt"
check strace_median_threaded "804 862 7" "$(wl_strace_median 1 "$t/m1.txt" "$t/m2.txt" "$t/m3.txt")"
check strace_median_single "812 862 0" "$(wl_strace_median 0 "$t/m1.txt")"
check strace_median_one_threaded "807 862 5" "$(wl_strace_median 1 "$t/m1.txt")"
check strace_empty "- - -" "$(: > "$t/0.txt"; wl_strace_sys "$t/0.txt" 1)"
# wl_count_run.sh feeds the thread flag (a clone trace) to the median and keeps the mtmem field in its cached row
check wl_count_run_detects_thread 1 "$(grep -c 'wl_has_thread "$tmp/clone.txt" && mt=1' "$here/wl_count_run.sh")"
check wl_count_run_passes_mt 1 "$(grep -c 'wl_strace_median $mt $files' "$here/wl_count_run.sh")"
check wl_count_run_row_fields 1 "$(grep -c "print NF }' \"\$cache\")\" = 10 \]" "$here/wl_count_run.sh")"

echo "wl_stats_test: $pass passed, $fail failed"
[ $fail = 0 ]
