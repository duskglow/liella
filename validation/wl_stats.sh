#!/bin/bash
# LAB-ONLY: this script drives the project's own test hosts and is not needed to build or test Liella.
# Statistics helpers for wl_count_run.sh (WL-FAULTS-STAT). Source this file.
#
# Page faults are not exact: on a NUMA host with numa_balancing one outlier run can double them, and the
# mean of `perf stat -r 3` then shows a false regression. So faults are reported as the MEDIAN of N runs
# (with min..max beside it); instructions stay a mean (they are exact, sd% is printed).
#
# (sources wl_syscls.sh for the syscall classification)
#   wl_stats      stdin: one run per line "instr_u instr_k page_faults"
#                 stdout: "instr_u_mean instr_u_sd% instr_k_mean faults_median faults_min faults_max"
#                 (even counts: the two middle values are averaged, rounded down)
#   wl_faults_differ NEW BASE [PCT]   exit 0 when |NEW-BASE| > PCT% of BASE (default 2), or BASE is 0 and
#                                      NEW is not; exit 1 otherwise
. "${WL_SYSCLS:-$(dirname "${BASH_SOURCE[0]}")/wl_syscls.sh}"

wl_stats() {
    awk '
        { n++; iu[n] = $1 + 0; ik[n] = $2 + 0; pf[n] = $3 + 0 }
        END {
            if (n == 0) { print "- - - - - -"; exit }
            for (i = 1; i <= n; i++) { su += iu[i]; sk += ik[i] }
            mu = su / n
            for (i = 1; i <= n; i++) v += (iu[i] - mu) ^ 2
            sd = (n > 1 && mu > 0) ? sqrt(v / (n - 1)) / mu * 100 : 0
            for (i = 2; i <= n; i++) { x = pf[i]; j = i - 1; while (j >= 1 && pf[j] > x) { pf[j + 1] = pf[j]; j-- } pf[j + 1] = x }
            med = (n % 2) ? pf[(n + 1) / 2] : int((pf[n / 2] + pf[n / 2 + 1]) / 2)
            printf "%.0f %.2f %.0f %d %d %d\n", mu, sd, sk / n, med, pf[1], pf[n]
        }'
}

wl_faults_differ() {
    awk -v a="$1" -v b="$2" -v p="${3:-2}" 'BEGIN {
        d = a - b; if (d < 0) d = -d
        exit (b + 0 > 0 ? (d * 100 > p * b ? 0 : 1) : (d > 0 ? 0 : 1))
    }'
}

# wl_strace_sys FILE [MT] -> "GATING FUTEX MTMEM" from an `strace -c -f` summary (WLCOUNT-FUTEX, WLCOUNT-MTMEM-2). It takes the
# classification from wl_syscls.sh (the same code wl_run.sh uses, so both tools agree): GATING = all calls minus the
# scheduling-dependent ones; FUTEX = futex calls; MTMEM = for a run that created threads (MT=1, found by wl_has_thread on a
# clone trace) its mmap/munmap/mremap/brk/mprotect/madvise calls. Futex counts and a threaded program's arena/stack mappings
# depend on scheduling and vary run to run with identical code, so only GATING is a deterministic counter; the other two
# are advisory. Prints "- - -" when the file has no total line.
wl_strace_sys() {
    if grep -q 'total' "$1"; then
        wl_classify "$1" "${2:-0}" | awk -F'\t' '{ printf "%d %d %d\n", $1, $10, $11 }'
    else
        echo "- - -"
    fi
}

# wl_strace_median MT FILE... -> "GATING FUTEX MTMEM": the median over several `strace -c -f` summaries of each number.
# Kept from WLCOUNT-FUTEX: it costs two extra runs and guards against a one-off strace hiccup in the gating number.
wl_strace_median() {
    local mt=$1 f
    shift
    for f in "$@"; do wl_strace_sys "$f" "$mt"; done | awk '
        { n++; a[n] = $1; b[n] = $2; c[n] = $3 }
        function med(x, m,   i, j, v) { for (i = 2; i <= m; i++) { v = x[i]; j = i - 1; while (j >= 1 && x[j] > v) { x[j + 1] = x[j]; j-- } x[j + 1] = v } return (m % 2) ? x[(m + 1) / 2] : int((x[m / 2] + x[m / 2 + 1]) / 2) }
        END { if (n == 0 || a[1] == "-") print "- - -"; else printf "%d %d %d\n", med(a, n), med(b, n), med(c, n) }'
}
