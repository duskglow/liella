#!/bin/bash
# LAB-ONLY: this script drives the project's own test hosts and is not needed to build or test Liella.
# Per-call instruction counts of libc functions, Liella vs glibc (WL-PERCALL). Counts, never time:
# instructions:u per call = (I(N calls) - I(0 calls)) / N, perf stat on keke, static binaries (glibc: gcc -static).
# perf sampling loses most samples on a VM; this is the exact alternative for "which function is the gap".
#
#   validation/wl_percall.sh [--base REV] [--rev REV] [--spec FILE.c] [--cases a,b,...] [--locale L] [--event E] <dir|rev>
#
# RUNS ON THE HOST WHERE IT IS STARTED (VALIDATION-JOBRUN, 2026-10-20; no ssh, no vm_run). From a role directory:
#   ~/bin/jobrun <label> count -- sh -c 'cd ~/w/<role> && bash validation/wl_percall.sh --spec X.c .'
# <dir|rev>: a Liella tree on this host. A role directory (has .synced, no .git): the revision is --rev REV, else the
#   `head=` of its .synced marker, else "unknown" (never an error; an "unknown" run is not cached across runs); a tree that
#   .synced calls dirty is refused unless --rev names it. A git worktree: its HEAD (dirty refused). Not a directory: a rev
#   of the git repository you run in. The tree is copied to ~/wl/src/<rev> (not target/, .git, .synced).
# --nowait is accepted and ignored (jobrun detaches). --rev REV: name the revision (cache key) of a role tree.
# --spec: a probe spec (C file; see validation/percall/percall.h: CASE(name, typical_calls, per_iteration) { setup;
#   loop of n calls }). Default: the built-in set validation/percall/builtin.c (fstatat fstat stat readdir
#   opendir_closedir malloc_free fopen_fclose fgets_line fwrite_short printf_short setlocale_query
#   strcoll_short getenv); validation/percall/locale.c = the locale-sensitive calls (mb*/wc*, ctype, strxfrm,
#   printf %ls/%lc, regex, iconv). --cases: only these (default all).
# --base REV: the "base" column (Liella at REV). In a git tree, default (TOOLS-15): `git merge-base NEW origin/next`; --base none
#   drops the column; a REV the branch is behind becomes merge-base NEW REV (WL_BASE_EXACT=1 keeps it; wl_base.sh).
#   Without git (a role directory) the default is the .synced `merge_base=` (sync_role --base writes it; none: no base
#   column), and an explicit REV must be a full sha (it wins; `base_sha=` in .synced is the base ref's tip). Either way its
#   source or products must be cached on this host (~/wl/src/<sha>, ~/wl/products/<sha>; prime them from a git checkout
#   with validation/wl_prime_base.sh <host> <sha>), else exit 2.
#   The base used and why is printed and recorded in the log (wl_base: line, read by finish.sh). Output: table sorted by (new-glibc) x typical calls
#   (calls = 0 in the spec: unsorted, last), per case glibc / base / new, new/glibc, sd over 5 runs (instructions).
# Products cached per sha in ~/wl/products/<sha> on the host,
# binaries and rows per sha and spec hash in ~/wl/percall/ (COUNT_FRESH=1 ignores cached rows).
# glibc is linked DYNAMICALLY (the system libc.so.6 with -z now; static glibc ignores the locale for collation) and
# runs in the same locale as Liella: --locale L (default en_US.UTF-8) sets LC_ALL, and the harness calls
# setlocale(LC_ALL, "") before every case (PERCALL-LOCALE; per-case override CASE_LOCALE in percall.h). The header
# prints each variant's active locale and codeset; a variant that is not in the requested locale stops the run (exit 5). Column `check`: every case's self-check line (percall.h) agrees between
# glibc and Liella (ok), differs (MISMATCH: that glibc number is no reference; details below the table) or is absent (-).
# --event E: count the perf event E (e.g. cycles:u, branch-misses:u) instead of instructions:u; same table, the
#   header names the event. Rows are cached per event. Only instructions:u is deterministic: other events on the keke
#   VM are advisory (read sd_new / sd_glibc). Unknown flags exit 2. Tracing one call instruction by instruction: validation/wl_trace.sh.
# Clean room: glibc is only ever measured as a black box (totals).
set -u
base=
spec=
cases=all
locale=en_US.UTF-8
event=instructions:u
nowait=0
trace=
rev=
while [ $# -gt 0 ]; do
    case $1 in
        --base) base=$2; shift 2 ;;
        --spec) spec=$2; shift 2 ;;
        --cases) cases=$2; shift 2 ;;
        --locale) locale=$2; shift 2 ;;
        --event) event=$2; shift 2 ;;
        --nowait) nowait=1; shift ;;   # accepted for old callers; jobrun detaches now
        --rev) rev=$2; shift 2 ;;
        --trace) trace=$2; shift 2 ;;   # FUNC:SKIP:MAX, set by wl_trace.sh
        -h|--help) sed -n '2,/^set -u/p' "$0" | sed '$d'; exit 0 ;;
        --) shift; break ;;
        -*) echo "wl_percall: unknown flag $1 (see --help)" >&2; exit 2 ;;
        *) break ;;
    esac
done
[ $# -eq 1 ] || { sed -n '2,/^set -u/p' "$0" | sed '$d'; exit 2; }
target=$1
case "$event" in *[!A-Za-z0-9_:.-]*|"") echo "wl_percall: bad event name $event" >&2; exit 2 ;; esac
here=$(cd "$(dirname "$0")" && pwd)
. "$here/wl_local.sh"
root=; gitmode=0
if [ -d "$target" ]; then
    if wl_is_git "$target"; then
        gitmode=1
        [ -z "$(git -C "$target" status --porcelain --untracked-files=no)" ] || { echo "wl_percall: $target has uncommitted changes; commit first"; exit 2; }
        new=${rev:-$(git -C "$target" rev-parse HEAD)} || exit 2
        repo=$target
    else
        root=$(wl_role_root "$target") || { echo "wl_percall: $target is neither a git tree nor a role directory (no .synced)"; exit 2; }
        wl_local_rev "$root" "$rev" || exit 2
    fi
else
    gitmode=1
    new=$(git rev-parse "$target") || exit 2
    repo=.
fi
[ $gitmode = 1 ] && srckey=$new
shas=$new
rec=
[ -n "$trace" ] && [ -n "$base" ] && [ "$base" != none ] && { echo "wl_trace: --base does not apply to a trace" >&2; exit 2; }
if [ -n "$trace" ] || [ "$base" = none ]; then base=-
else
    if [ $gitmode = 1 ]; then
        . "$here/wl_base.sh"
        wl_resolve_base "$repo" "$new" "$base" || exit 2
    else
        wl_local_base "$base" "$root" || exit 2
        echo "wl_base: ${base:0:9} - $base_why"
    fi
    [ "$base" = - ] || { shas="$new $base"; rec="echo 'wl_base: $base mb=$base_mb' && "; }
fi
t0=$(date +%s)
if [ -n "$spec" ]; then
    [ -f "$spec" ] || { echo "wl_percall: no spec $spec"; exit 2; }
    sname=u$(md5sum < "$spec" | cut -c1-8); sfile=$spec
else
    sname=builtin; sfile=$here/percall/builtin.c
fi
# UNIQUE LABEL (TOOLS-12): +$$ - two runs of one sha started in the same second got the same label; two runs of one sha started in the same second shared a log.
label="wlp-${new:0:9}-$sname-$(date +%H%M%S)-$$"
[ -n "$trace" ] && { [ "$base" = - ] || { echo "wl_trace: --base does not apply to a trace" >&2; exit 2; }; label="wlt-${new:0:9}-$(date +%H%M%S)-$$" ; }
case $locale in *[!A-Za-z0-9._@-]*|'') echo "wl_percall: bad locale name '$locale'" >&2; exit 2 ;; esac
# prep: the spec, the harness, the wl_* scripts and a source tree per sha not on this host yet. Sets B and cmd.
prep() {
    mkdir -p ~/wl/src ~/wl/products ~/logs ~/wl/percall/spec/$sname || return 1
    cp "$here/percall/percall.h" "$here/percall/harness.c" ~/wl/percall/spec/$sname/ || return 1
    cp "$sfile" ~/wl/percall/spec/$sname/spec.c || return 1
    wl_install_local || return 1   # B = ~/wl/bin/v-<hash>, never overwritten
    for s in $shas; do
        if [ "$s" = "$new" ]; then k=$srckey; else k=$s; fi
        [ -f ~/wl/products/$k/libc.a ] || [ -d ~/wl/src/$k ] && continue
        if [ $gitmode = 1 ]; then
            t=~/wl/src/.$k.$$; rm -rf "$t"; mkdir -p "$t" && git -C "$repo" archive --format=tar "$s" | tar -x -C "$t" && { mv -T "$t" ~/wl/src/$k 2>/dev/null || rm -rf "$t"; } || return 1
        else
            wl_local_src "$root" "$k" || return 1
        fi
    done
    cmd="${rec}WL_EVENT=$event COUNT_FRESH=${COUNT_FRESH:-0} PC_LOCALE=$locale bash $B/wl_percall_run.sh $sname $cases $base $srckey $trace"
}
[ $nowait = 1 ] && echo "wl_percall: --nowait ignored (this tool runs where it is started; jobrun detaches)" >&2
prep || exit 1
wl_local_run "$label" "$cmd"
rc=$(cat ~/logs/$label.done)
# WLP_TAIL=0 prints the whole log (wl_stakes report needs every row; the default tail of 60 dropped the top rows).
if [ "${WLP_TAIL:-60}" = 0 ]; then cat ~/logs/$label.log; else tail -n ${WLP_TAIL:-60} ~/logs/$label.log; fi
echo "wl_percall: exit $rc, $(( $(date +%s) - t0 )) s wall (log ~/logs/$label.log; rev $new)"
exit "$rc"
