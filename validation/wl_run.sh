#!/bin/bash
# LAB-ONLY: this script drives the project's own test hosts and is not needed to build or test Liella.
# Workload gate, step 2 (runs on a timing host, or keke): measure every workload for each variant.
#
#   wl_run.sh OUT.tsv VARIANT...        (variants: glibc, <sha>, ...; kits in $WL/kit/VARIANT/bin)
#
# WL (default ~/wl) holds kit/ and work/ (shipped by wl_gate.sh). ONLY=REGEX limits the
# workloads. A variant may be written NAME@TAG (e.g. abc123@2): the kit of NAME, measured again
# under another label (A/A runs, see wl_aa.py). STRACE=0 skips the syscall counts.
# Per workload and variant: one discarded warm-up run, then
#   perf stat -r 5 -x, -e task-clock,instructions:u,page-faults   (CPU time, user instructions
#       of the run and its children, page faults; no sudo: perf_event_paranoid <= 1 suffices),
#   one run through md5sum (the output check), and
#   one `strace -c -f` run (syscall counts, summed into the classes below).
# If a variant's output differs from the variant glibc's, both are produced again into files and
# a fingerprint of the differing lines (count:sha1) is recorded; wl_known_diffs.tsv lists the
# fingerprints that are allowed.
# OUT.tsv columns: workload variant cpu_s sd_pct md5 instr instr_sd pf pf_sd sys_total sys_rw
#   sys_open sys_fstat sys_stat sys_dir sys_mem sys_proc sys_other dfp cycles cycles_u cycles_k sys_futex sys_mtmem
#   (cycles = user+kernel, FALLBACK-CYCLES: advisory with cpu_s; the report prints the GHz = cycles / cpu time;
#    cycles_u, cycles_k = the same event with the :u / :k modifier, WLRUN-CYCLES-UK: which side a gap is on, program/libc
#    user time or kernel time (page faults, syscalls). New columns are appended, older files without them still load.)
# Syscall classes: rw = read/write family; open = open*/close/dup*/pipe*/fcntl/ioctl; fstat =
# fstat, newfstatat, statx; stat = stat/lstat/access/readlink; dir = getdents*; mem = mmap/
# munmap/mremap/brk/mprotect/madvise; proc = clone/fork/vfork/execve/wait/exit/kill/signals;
# other = the rest. SCHEDULING-DEPENDENT CALLS (WLRUN-FUTEX, 2026-10-21; owner rule: WL gates fail only on deterministic
# counters) are reported apart and are NOT in sys_total / sys_mem / sys_other:
#   sys_futex = all futex calls (how often a thread had to wait depends on timing; glibc and Liella both vary ~15% run to run);
#   sys_mtmem = when the run created a thread (a clone/clone3 with CLONE_THREAD, found by a second, tiny strace run), ALL its
#       mmap/munmap/mremap/brk/mprotect/madvise calls. Measured on a 4-thread sort: mmap 22..23 and munmap 3..6 between
#       identical runs, because a worker thread sometimes gets its own allocator arena (one more 32 MiB mmap + two trims) and
#       because the alignment trims of an arena depend on the address the kernel returns while other threads map at the same
#       time. Single-threaded workloads keep sys_mem in the gate. sys_futex and sys_mtmem are advisory (the report prints
#       the futex change) and never a verdict. Still counted for threaded runs: rw, open, fstat, stat, dir, proc (clone,
#       exit, signals, wait), other (without futex), and the total of those. Variants run in the given order, interleaved per workload, so a drifting
# host load hits all of them alike. The gate fails only on the counters (instr, pf, sys_*) and
# on unexplained output differences; cpu_s is advisory.
set -u
WL=${WL:-$HOME/wl}
out=$1
shift
. "$(dirname "$0")/wl_workloads.sh"
work=$WL/work
tmp=$WL/tmp.$$
mkdir -p "$tmp"
trap 'rm -rf "$tmp"' EXIT
[ -d "$work/tree" ] || { mkdir -p "$work/tree" && tar -xf "$work/tree.tar" -C "$work/tree"; }
printf 'workload\tvariant\tcpu_s\tsd_pct\tmd5\tinstr\tinstr_sd\tpf\tpf_sd\tsys_total\tsys_rw\tsys_open\tsys_fstat\tsys_stat\tsys_dir\tsys_mem\tsys_proc\tsys_other\tdfp\tcycles\tcycles_u\tcycles_k\tsys_futex\tsys_mtmem\n' > "$out"

. "${WL_SYSCLS:-$(dirname "$0")/wl_syscls.sh}"   # the classifier, shared with wl_count_run.sh (WLCOUNT-MTMEM-2)
nosys=$(printf -- '-\t-\t-\t-\t-\t-\t-\t-\t-')
nosys2=$(printf -- '-\t-')

ev() { # event field: first line of the perf stat output naming the event, field $2 of the csv
    echo "$stat" | grep ",$1," | tail -1 | cut -d, -f"$2"
}

for d in "${WL_DEFS[@]}"; do
    name=${d%%|*}
    cmd=${d#*|}
    [[ $name =~ ${ONLY:-.} ]] || continue
    refmd5=
    refcmd=
    for v in "$@"; do
        c=${cmd//@/$WL/kit/${v%%@*}/bin}
        (cd "$work" && sh -c "$c" > /dev/null 2>&1)    # warm-up
        stat=$(cd "$work" && perf stat -r 5 -x, -e task-clock,instructions:u,page-faults,cycles,cycles:u,cycles:k sh -c "$c" 2>&1 > /dev/null)
        ms=$(ev task-clock 1)
        sum=$(cd "$work" && sh -c "$c" 2>/dev/null | md5sum | cut -c1-12)
        sys=$nosys
        sysx=$nosys2
        if [ "${STRACE:-1}" = 1 ]; then
            (cd "$work" && strace -c -f -o "$tmp/strace.txt" sh -c "$c" > /dev/null 2>&1)
            (cd "$work" && strace -f -e trace=clone,clone3 -o "$tmp/clone.txt" sh -c "$c" > /dev/null 2>&1)
            mt=0
            wl_has_thread "$tmp/clone.txt" && mt=1
            all=$(wl_classify "$tmp/strace.txt" "$mt")
            sys=$(echo "$all" | cut -f1-9)
            sysx=$(echo "$all" | cut -f10-11)
        fi
        dfp=-
        if [ "$v" = glibc ]; then
            refmd5=$sum
            refcmd=$c
        elif [ -n "$refmd5" ] && [ "$sum" != "$refmd5" ]; then
            (cd "$work" && sh -c "$refcmd" > "$tmp/ref.out" 2>/dev/null; sh -c "$c" > "$tmp/new.out" 2>/dev/null)
            dfp="$(diff -a "$tmp/ref.out" "$tmp/new.out" | grep -a -c '^[<>]'):$(diff -a "$tmp/ref.out" "$tmp/new.out" | grep -a '^[<>]' | sha1sum | cut -c1-12)"
            rm -f "$tmp/ref.out" "$tmp/new.out"
        fi
        printf '%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\n' "$name" "$v" "$(echo "scale=3; $ms / 1000" | bc)" "$(ev task-clock 4)" "$sum" \
            "$(ev instructions:u 1)" "$(ev instructions:u 4)" "$(ev page-faults 1)" "$(ev page-faults 4)" "$sys" "$dfp" "$(ev cycles 1)" "$(ev cycles:u 1)" "$(ev cycles:k 1)" "$sysx" >> "$out"
    done
done
echo "wl_run: $(($(wc -l < "$out") - 1)) rows in $out"
