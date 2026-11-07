#!/bin/bash
# LAB-ONLY: this script drives the project's own test hosts and is not needed to build or test Liella.
# Syscall classification shared by wl_run.sh and wl_count_run.sh (WLCOUNT-MTMEM-2; the rule is WLRUN-FUTEX's). Source this file.
#
# Owner rule: workload gates fail only on deterministic counters. Two kinds of calls depend on scheduling and are reported
# apart (never in the gating total):
#   futex  - all futex calls (how often a thread had to wait depends on timing);
#   mtmem  - when the run created a thread (a clone/clone3 with CLONE_THREAD), ALL its mmap/munmap/mremap/brk/mprotect/madvise
#            calls (a worker sometimes gets its own allocator arena; the alignment trims of an arena depend on the address
#            the kernel returns while other threads map at the same time). Single-threaded runs keep them in "mem".
#
#   wl_has_thread CLONEFILE     exit 0 when the `strace -f -e trace=clone,clone3` output in CLONEFILE shows CLONE_THREAD
#   wl_has_child CLONEFILE      exit 0 when the `strace -f -e trace=clone,clone3,fork,vfork` output shows a new process
#   wl_classify STRACEFILE MT [MP]  stdout (tab separated, one line): total(gating) rw open fstat stat dir mem proc other futex
#                               mtmem piperw, from an `strace -C -f -y` (or `strace -c -f`) output; MT = 1 when the run created
#                               threads, MP = 1 when it created another process (default 0)
# WL_CLASSIFY is the awk program (variables mt = the thread flag, mp = the process flag); wl_run_test.sh and wl_stats_test.sh
# test it directly. Lines before the "% time" header are trace lines (strace -C), the rows after it the summary.
WL_CLASSIFY='
!sum && /^% time/ { sum = 1; next }
!sum { if (mp && $0 ~ /^([0-9]+ )?(read|write|pread64|pwrite64|readv|writev|preadv|pwritev|sendfile|copy_file_range|splice|lseek)\([0-9]+<pipe:\[/) pp++; next }
$NF == "total" { total = $4; next }
$0 ~ /^-+/ { next }
{ n = $4; s = $NF }
s ~ /^(read|write|pread64|pwrite64|readv|writev|preadv|pwritev|sendfile|copy_file_range|splice|lseek)$/ { rw += n; next }
s ~ /^(open|openat|openat2|close|close_range|dup|dup2|dup3|pipe|pipe2|creat|fcntl|ioctl)$/ { op += n; next }
s ~ /^(fstat|newfstatat|statx|fstatat64)$/ { fs += n; next }
s ~ /^(stat|lstat|access|faccessat|faccessat2|readlink|readlinkat)$/ { st += n; next }
s ~ /^(getdents|getdents64)$/ { dr += n; next }
s == "futex" { fx += n; next }
s ~ /^(mmap|munmap|mremap|brk|mprotect|madvise)$/ { if (mt) mx += n; else mm += n; next }
s ~ /^(clone|clone3|fork|vfork|execve|wait4|waitid|exit|exit_group|kill|tgkill|rt_sigaction|rt_sigprocmask|rt_sigreturn)$/ { pr += n; next }
{ ot += n }
END { if (pp > rw) pp = rw; printf "%d\t%d\t%d\t%d\t%d\t%d\t%d\t%d\t%d\t%d\t%d\t%d", total - fx - mx - pp, rw - pp, op, fs, st, dr, mm, pr, ot, fx, mx, pp }'

wl_has_thread() { grep -q 'CLONE_THREAD' "$1"; }

wl_has_child() { grep -E '(^|[0-9] )(clone3?|v?fork)\(' "$1" | grep -qv 'CLONE_THREAD'; }

wl_classify() { # STRACEFILE MT [MP]
    sed 's/^ *//; s/  */ /g' "$1" | awk -v mt="${2:-0}" -v mp="${3:-0}" "$WL_CLASSIFY"
}
