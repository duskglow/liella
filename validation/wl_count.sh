#!/bin/bash
# LAB-ONLY: this script drives the project's own test hosts and is not needed to build or test Liella.
# One-call deterministic workload counters for a branch (WL-COUNT). Counts, never time: instructions:u,
# instructions:k, page faults, syscalls and an output check, for glibc / BASE / NEW, measured on keke.
#
#   validation/wl_count.sh [--base REV] [--rev REV] <dir|rev> <workload>...
#
# RUNS ON THE HOST WHERE IT IS STARTED (VALIDATION-JOBRUN, 2026-10-20; no ssh, no vm_run, no --host: dispatch is the
# caller's). From a role directory:
#   ~/bin/jobrun <label> count -- sh -c 'cd ~/w/<role> && bash validation/wl_count.sh --base <full sha> . seq'
# <dir|rev>: a Liella tree on this host. A role directory (has .synced, no .git): the revision is --rev REV, else the
#   `head=` of its .synced marker, else "unknown" (never an error; not cached across runs); a tree .synced calls dirty is
#   refused unless --rev names it. WITHOUT git the base is the role's .synced `merge_base=` (written by sync_role --base; `base_sha=` is the base ref's
#   tip), or --base <full sha> (wins). Its source or products must be cached here (~/wl/src/<sha>, ~/wl/products/<sha>;
#   prime with validation/wl_prime_base.sh <host> <sha> from a git checkout), else exit 2 (the base column is not optional for counts).
#   A git worktree directory (its HEAD; dirty refused) or any rev of the repository you run in works as before. BASE defaults to `git merge-base NEW origin/next` (TOOLS-15; a --base
#   REV the branch is behind becomes merge-base NEW REV, WL_BASE_EXACT=1 keeps it; see wl_base.sh); the base
#   used and why is printed and recorded in the log (wl_base: line, read by finish.sh). Workload names are those of
#   validation/wl_workloads.sh (seq, du, gawk-sum, sort-C, ...).
# What runs (all on this host):
#   1. NEW (role tree copy or git archive) and BASE (git archive) to ~/wl/src/<sha> (skipped when present);
#   2. wl_prep.sh with WL_PROGS = only the programs the named workloads use: cargo xtask so for each sha
#      (cached in ~/wl/products/<sha>) and a relink of just those programs (cached in ~/wl/kit/<sha>);
#   3. wl_count_run.sh: perf stat -r 3 + strace -c + md5 per workload and variant; rows cached per
#      variant in ~/wl/count/<sha>/ (BASE results are reused by every later call).
# Prints one table per workload: new, base, glibc, new/glibc, new/base, SAME/DIFF output.
# SYSCALLS EXCLUDE FUTEX (WLCOUNT-FUTEX, 2026-10-21): the "syscalls-gating" row is the gating syscall number (it also excludes the
#   threaded-memory calls, see THREADED MEMORY below). Futex
#   wait/wake counts of threaded workloads (sort-C, sort-utf8) depend on scheduling and differ run to run with identical
#   code, so they are reported on their own "futex (advisory)" row and never decide a verdict. Rows cached before this
#   change (fewer than 9 fields) are re-measured. Each variant's strace count is the median of three runs: with futex
#   removed a threaded program's mmap/mprotect/munmap count still moved by a few calls (803..807, 4-thread sort).
# THREADED MEMORY (WLCOUNT-MTMEM-2, 2026-10-21; the same rule as wl_run.sh, one classifier: validation/wl_syscls.sh): when a
#   workload creates a thread (a clone/clone3 with CLONE_THREAD), ALL its mmap/munmap/mremap/brk/mprotect/madvise calls are
#   the advisory row "mtmem (advisory)" and not in "syscalls-gating" (arena and stack set-up of threads depends on
#   scheduling). Single-threaded workloads keep them in the gating row. Rows cached before this change (fewer than 10 fields)
#   are re-measured.
# --nowait: accepted and ignored (jobrun detaches). Env: COUNT_FRESH=1 ignores cached rows; FAULT_PCT (2) = faults difference that triggers a fresh re-measure.
# WLC_ENV='NAME=VALUE ...' (SSE2-GAP-4, like wl_percall's WLP_ENV): environment for the measured program, applied
#   to ALL THREE columns (glibc, base, new) - e.g. WLC_ENV='GLIBC_TUNABLES=glibc.cpu.hwcaps=-AVX2' forces the
#   SSE2 variants in glibc AND Liella (both read the tunable). Rows measured under an env are cached apart.
#   Self-check: with WLC_ENV set, the glibc row is also measured without it and the table prints whether the
#   glibc column changed (a "WARNING: glibc column unchanged" means the env did not reach glibc). Before this
#   fix the glibc row came from a cache keyed only by "glibc", so an "SSE2-forced" run compared Liella SSE2
#   with glibc AVX2.
# Output: the measured program's stdout goes into a pipe (| cat), never straight to /dev/null: GNU grep
#   stops at the first match when its stdout is /dev/null (and other programs may special-case it).
set -u
base=
nowait=0
rev=
while [ $# -gt 0 ]; do
    case $1 in
        --base) base=$2; shift 2 ;;
        --nowait) nowait=1; shift ;;
        --rev) rev=$2; shift 2 ;;
        --host) echo "wl_count: --host is gone: this tool runs where it is started; start it with jobrun on that host" >&2; exit 2 ;;
        -h|--help) sed -n '2,/^set -u/p' "$0" | sed '$d'; exit 0 ;;
        --) shift; break ;;
        -*) echo "wl_count: unknown flag $1 (see --help)" >&2; exit 2 ;;
        *) break ;;
    esac
done
[ $# -ge 2 ] || { sed -n '2,/^set -u/p' "$0" | sed '$d'; exit 2; }
target=$1
shift
here=$(cd "$(dirname "$0")" && pwd)
. "$here/wl_local.sh"
root=; gitmode=0
if [ -d "$target" ]; then
    if wl_is_git "$target"; then
        gitmode=1
        [ -z "$(git -C "$target" status --porcelain --untracked-files=no)" ] || { echo "wl_count: $target has uncommitted changes; commit first"; exit 2; }
        new=${rev:-$(git -C "$target" rev-parse HEAD)} || exit 2
        repo=$target
    else
        root=$(wl_role_root "$target") || { echo "wl_count: $target is neither a git tree nor a role directory (no .synced)"; exit 2; }
        wl_local_rev "$root" "$rev" || exit 2
    fi
else
    gitmode=1
    new=$(git rev-parse "$target") || exit 2
    repo=.
fi
[ $gitmode = 1 ] && srckey=$new
if [ $gitmode = 1 ]; then
    . "$here/wl_base.sh"
    wl_resolve_base "$repo" "$new" "$base" || exit 2
else
    wl_local_base "$base" "$root" || exit 2
    [ "$base" != - ] || { echo "wl_count: no base: sync the role with --base (.synced merge_base) or pass --base <full sha> (cached on this host)" >&2; exit 2; }
    echo "wl_base: ${base:0:9} - $base_why"
fi
rec="echo 'wl_base: $base mb=$base_mb' && "
ns=${srckey:0:9}
wenv=${WLC_ENV:-}
for kv in $wenv; do
    [[ $kv =~ ^[A-Za-z_][A-Za-z0-9_]*=[A-Za-z0-9_.,:=/+-]*$ ]] || { echo "wl_count: WLC_ENV entry '$kv' is not NAME=VALUE (plain characters only)" >&2; exit 2; }
done
bs=${base:0:9}
. "$here/wl_workloads.sh"
progs=
for w in "$@"; do
    hit=0
    for d in "${WL_DEFS[@]}"; do
        [ "${d%%|*}" = "$w" ] || continue
        hit=1
        for p in $(echo "${d#*|}" | grep -o '@/[a-z0-9]*' | cut -c3-); do
            case " $progs " in *" $p "*) ;; *) progs="$progs $p" ;; esac
        done
    done
    [ $hit = 1 ] || { echo "wl_count: unknown workload $w"; exit 2; }
done
progs=${progs# }
list=$(IFS=,; echo "$*")
t0=$(date +%s)

# UNIQUE LABEL (TOOLS-12): +$$ - two runs of one sha started in the same second must not share a log.
label="wlc-$ns-$(date +%H%M%S)-$$"
# prep: the wl_* scripts and a source tree per sha not on this host yet. Sets B and cmd.
prep() {
    mkdir -p ~/wl/bin ~/wl/out ~/wl/src ~/logs || return 1
    wl_install_local || return 1   # B = ~/wl/bin/v-<hash>, never overwritten
    for s in $new $base; do
        if [ "$s" = "$new" ]; then k=$srckey; else k=$s; fi
        [ -f ~/wl/products/$k/libc.a ] || [ -d ~/wl/src/$k ] && continue
        if [ $gitmode = 1 ]; then
            t=~/wl/src/.$k.$$; rm -rf "$t"; mkdir -p "$t" && git -C "$repo" archive --format=tar "$s" | tar -x -C "$t" && { mv -T "$t" ~/wl/src/$k 2>/dev/null || rm -rf "$t"; } || return 1
        else
            wl_local_src "$root" "$k" || return 1
        fi
    done
    cmd="${rec}WL_PROGS='$progs' bash $B/wl_prep.sh $srckey $base && WLC_ENV='$wenv' COUNT_FRESH=${COUNT_FRESH:-0} FAULT_PCT=${FAULT_PCT:-2} bash $B/wl_count_run.sh $list $base $srckey"
}
[ $nowait = 1 ] && echo "wl_count: --nowait ignored (this tool runs where it is started; jobrun detaches)" >&2
prep || exit 1
wl_local_run "$label" "$cmd"
rc=$(cat ~/logs/$label.done)
grep -v '^wl_prep: done' ~/logs/$label.log | tail -n 200
echo "wl_count: exit $rc, $(( $(date +%s) - t0 )) s wall (log ~/logs/$label.log; rev $new)"
exit "$rc"
