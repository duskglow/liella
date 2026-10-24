# LAB-ONLY: this script drives the project's own test hosts and is not needed to build or test Liella.
# Base selection shared by wl_count.sh and wl_percall.sh (TOOLS-15). Sourced, not run.
#
# wl_resolve_base <repo> <new-sha> <requested REV or "">: sets base, base_why, base_mb.
#   ""  (no --base): base = git merge-base NEW origin/next (WL_NEXT_REF overrides the ref). A branch cut from next
#       and behind its tip would otherwise be compared with commits it does not contain: next's newer changes
#       were counted as the branch's own (GETTEXT-HIT-3 lost a whole run to this).
#   REV that is an ancestor of NEW: used as given.
#   REV that is NOT an ancestor of NEW (the branch is behind REV): base = merge-base NEW REV, for the same reason.
#       WL_BASE_EXACT=1 keeps REV as given (deliberate cross-branch comparison).
# Prints one line "wl_base: <sha> - <why>" on stdout. base_mb = merge-base of NEW and the next ref ("-" if none);
# the remote log records both so finish.sh can flag a base that is not the fork point.
wl_resolve_base() {
    local repo=$1 new=$2 req=$3 ref=${WL_NEXT_REF:-origin/next} r mb
    base_mb=$(git -C "$repo" merge-base "$new" "$ref" 2>/dev/null) || base_mb=-
    if [ -z "$req" ]; then
        [ "$base_mb" != - ] || { echo "wl_base: no merge-base of ${new:0:9} and $ref (fetch $ref, or pass --base)" >&2; return 2; }
        base=$base_mb
        if [ "$base" = "$(git -C "$repo" rev-parse "$ref")" ]; then
            base_why="default: $ref tip (HEAD contains it)"
        else
            base_why="default: merge-base of HEAD and $ref (branch is behind $ref; its tip would count next's newer commits as yours)"
        fi
    else
        r=$(git -C "$repo" rev-parse --verify --quiet "$req^{commit}") || { echo "wl_base: unknown rev $req" >&2; return 2; }
        if git -C "$repo" merge-base --is-ancestor "$r" "$new" || [ "${WL_BASE_EXACT:-0}" = 1 ]; then
            base=$r; base_why="--base $req as given"
        else
            mb=$(git -C "$repo" merge-base "$new" "$r") || { echo "wl_base: ${new:0:9} and $req share no history" >&2; return 2; }
            base=$mb
            base_why="merge-base of HEAD and $req (branch is behind $req; WL_BASE_EXACT=1 compares with $req itself)"
        fi
    fi
    echo "wl_base: ${base:0:9} - $base_why"
}
