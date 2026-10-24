#!/bin/bash
# LAB-ONLY: this script drives the project's own test hosts and is not needed to build or test Liella.
# Allocation trace of one workload's program under gdb on keke (ALLOC-TRACE). Answers "what does this program ask the
# allocator for, and how": per-size-class call counts, realloc chains (old size -> new size, moved or in place) and the
# top call sites. HEAP-REUSE settled in two calls what fault attribution had guessed wrong. Counts, never time.
#
#   validation/wl_alloctrace.sh [--top N] [--prog P] [--nowait] <worktree|rev> <workload>
#
# <worktree|rev>: a Liella worktree (its HEAD; dirty tree refused) or a rev of the repository you run in.
# <workload>: a name of validation/wl_workloads.sh (tar-c, ls-lR, ...). The workload command runs unchanged, but the FIRST
#   invocation of its main program (--prog P, default the first @/prog in the command) runs the kit binary directly under
#   gdb (not the sh wrapper, not glibc: Liella only). Later invocations (a loop of 10) run untraced.
# --top N: rows per table (default 10). Env TRMIN (default 256): smallest malloc whose realloc chain is followed; smaller
#   blocks show up in a chain only from their first realloc ("?"). TRSECS (default 240): time cap on the trace;
#   when it hits, the program is stopped and the summary says TRUNCATED (counts are then a prefix of the run). The summary has: calls and MB by size class per entry
#   point (malloc calloc realloc memalign..), free count, realloc moved/in-place counts, chains (identical chains are
#   merged: "N x m2187 -> 3280 moved -> 4920 in-place ..."), top call sites per entry point as caller < parent <
#   grandparent (gdb frame names, demangled when rustfilt is on keke). Tracing costs ~100 us per allocator call; the
#   program's own output is discarded. Needs gdb on keke.
# --nowait: start detached and print the re-attach command. Otherwise queued through $S/tools/vm_run.sh (S set) and waited.
# The kit is built by wl_prep.sh for just that program (cached per sha in keke:~/wl/kit/<sha>).
set -u
nowait=0 top=10 prog=
while [ $# -gt 0 ]; do
    case $1 in
        --top) top=$2; shift 2 ;;
        --prog) prog=$2; shift 2 ;;
        --nowait) nowait=1; shift ;;
        -h|--help) sed -n '2,/^set -u/p' "$0" | sed '$d'; exit 0 ;;
        -*) echo "wl_alloctrace: unknown flag $1 (see --help)" >&2; exit 2 ;;
        *) break ;;
    esac
done
[ $# -eq 2 ] || { sed -n '2,/^set -u/p' "$0" | sed '$d'; exit 2; }
target=$1 want=$2
case $top in *[!0-9]*|'') echo "wl_alloctrace: bad --top" >&2; exit 2 ;; esac
case $prog in *[!a-z0-9]*) echo "wl_alloctrace: bad --prog" >&2; exit 2 ;; esac
case $want in *[!A-Za-z0-9._-]*|'') echo "wl_alloctrace: bad workload name" >&2; exit 2 ;; esac
here=$(cd "$(dirname "$0")" && pwd)
. "$here/wl_workloads.sh"
cmd=
for d in "${WL_DEFS[@]}"; do [ "${d%%|*}" = "$want" ] && cmd=${d#*|}; done
[ -n "$cmd" ] || { echo "wl_alloctrace: unknown workload $want"; exit 2; }
[ -n "$prog" ] || prog=$(echo "$cmd" | grep -o '@/[a-z0-9]*' | head -1 | cut -c3-)
if [ -d "$target" ]; then
    [ -z "$(git -C "$target" status --porcelain --untracked-files=no)" ] || { echo "wl_alloctrace: $target has uncommitted changes; commit first"; exit 2; }
    new=$(git -C "$target" rev-parse HEAD) || exit 2
    repo=$target
else
    new=$(git rev-parse "$target") || exit 2
    repo=.
fi
keke="ssh -o BatchMode=yes claude@10.0.100.53"
t0=$(date +%s)
$keke "mkdir -p ~/wl/bin ~/wl/src ~/logs" || exit 1
. "$here/wl_install.sh"; wl_install_scripts || exit 1   # B = ~/wl/bin/v-<hash>, never overwritten
$keke "test -f ~/wl/products/$new/libc.a || test -d ~/wl/src/$new" || \
    { git -C "$repo" archive --format=tar "$new" | $keke "t=~/wl/src/.$new.\$\$; rm -rf \$t; mkdir -p \$t && tar -x -C \$t && { mv -T \$t ~/wl/src/$new 2>/dev/null || rm -rf \$t; }"; }
# UNIQUE LABEL (TOOLS-12): +$$ - two runs of one sha started in the same second got the same label; vm_run refused the
# second (label busy) and this script then waited on and printed the FIRST run's log as its own.
label="wla-${new:0:9}-$(date +%H%M%S)-$$"
rcmd="WL_PROGS='$prog' bash $B/wl_prep.sh $new && TRMIN='${TRMIN:-256}' TRSECS='${TRSECS:-240}' bash $B/wl_alloctrace_run.sh $new $want '$prog' $top"
if [ -n "${S:-}" ] && [ -f "$S/tools/vm_run.sh" ]; then
    VM_RUN_SIZE=S VM_NOWAIT=1 bash "$S/tools/vm_run.sh" keke "$label" "$rcmd" | tail -1
    [ "${PIPESTATUS[0]}" = 0 ] || { echo "wl_alloctrace: vm_run did not start $label (message above); not waiting on it" >&2; exit 2; }
    if [ $nowait = 1 ]; then echo "re-attach: bash \$S/tools/vm_wait.sh keke $label tail   (log keke:~/logs/$label.log)"; exit 0; fi
    while VM_WAIT_MAX_MIN=4 bash "$S/tools/vm_wait.sh" keke "$label" tail > "${TMPDIR:-/tmp}/wla.$$" 2>&1; [ $? = 3 ]; do :; done
    rm -f "${TMPDIR:-/tmp}/wla.$$"
else
    . "$here/wl_fallback.sh"; wl_fb_warn wl_alloctrace
    cmd=$rcmd
    wl_fb_start || { echo "wl_alloctrace: could not start $label on keke" >&2; exit 1; }
    if [ $nowait = 1 ]; then echo "started $label"; wl_fb_reattach; exit 0; fi
    wl_fb_wait || exit $?
fi
rc=$($keke "cat ~/logs/$label.done")
$keke "grep -v '^wl_prep: done' ~/logs/$label.log | tail -n 120"
echo "wl_alloctrace: exit $rc, $(( $(date +%s) - t0 )) s wall (log keke:~/logs/$label.log)"
exit "$rc"
