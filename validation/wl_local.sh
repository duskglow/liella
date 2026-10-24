# LAB-ONLY: this script drives the project's own test hosts and is not needed to build or test Liella.
# Sourced by wl_percall.sh and wl_count.sh (VALIDATION-JOBRUN, 2026-10-20): the tool runs ON the host where it is started
# (a role directory ~/w/<role> under `~/bin/jobrun <label> count -- sh -c 'cd ~/w/<role> && validation/wl_percall.sh ...'`);
# there is no ssh/scp/vm_run any more, remote dispatch is the caller's job. Needs: $here (the validation dir).
#
# A role directory has no .git (sync_role.sh copies files, not history) but a marker file <role>/.synced with key=value
# lines (tree=<treehash>-<clean|dirtyhash>, head=<commit>, base=<ref name>, base_sha=<sha of the base ref>,
# merge_base=<merge-base of head and base_sha>, branch=, story=, time=, source=).

wl_is_git() { git -C "$1" rev-parse --git-dir >/dev/null 2>&1; }   # wl_is_git <dir>

# wl_role_root <dir>: the role root (the dir holding .synced) at or above <dir>, or nothing.
wl_role_root() {
    local d
    d=$(cd "$1" 2>/dev/null && pwd) || return 1
    while [ "$d" != / ]; do [ -f "$d/.synced" ] && { echo "$d"; return 0; }; d=$(dirname "$d"); done
    return 1
}

wl_synced_get() { sed -n "s/^$2=//p" "$1/.synced" 2>/dev/null | head -1; }   # wl_synced_get <root> <key>

# wl_local_rev <root> <explicit --rev or ""> : sets new (the revision NAME and cache key) and new_why.
# Order: --rev, then .synced head, else "unknown" (never fails). A tree that .synced calls dirty is refused unless --rev
# names it (products are cached per revision; a dirty tree under the commit's name would poison the cache).
wl_local_rev() {
    local root=$1 rev=$2 tr
    if [ -n "$rev" ]; then new=$rev; new_why="--rev"
    else
        new=$(wl_synced_get "$root" head); tr=$(wl_synced_get "$root" tree)
        if [ -z "$new" ]; then new=unknown; new_why="no .synced head"
        else
            case $tr in *-clean|"") new_why=".synced head" ;;
                *) echo "wl: $root is synced dirty (tree=$tr); commit, sync again, or name it with --rev REV" >&2; return 2 ;;
            esac
        fi
    fi
    case $new in *[!A-Za-z0-9._-]*|"") echo "wl: bad revision name '$new'" >&2; return 2 ;; esac
    # "unknown" cannot be a cache key shared between different trees: give each run its own.
    srckey=$new
    [ "$new" = unknown ] && srckey=unknown-$(date +%y%m%d%H%M%S)-$$
    return 0
}

# wl_local_src <root> <key>: ~/wl/src/<key> = a copy of the role tree (unless the sources or the products are already there).
wl_local_src() {
    local root=$1 key=$2 t
    [ -f "$HOME/wl/products/$key/libc.a" ] || [ -d "$HOME/wl/src/$key" ] && return 0
    t=$HOME/wl/src/.$key.$$; rm -rf "$t"; mkdir -p "$t" || return 1
    tar -C "$root" --exclude=./target --exclude=./.git --exclude=./.synced --exclude=./.released --exclude=./.changed \
        --exclude=./.changed.hdr -cf - . | tar -x -C "$t" || { rm -rf "$t"; return 1; }
    mv -T "$t" "$HOME/wl/src/$key" 2>/dev/null || rm -rf "$t"
    [ -d "$HOME/wl/src/$key" ]
}

# wl_local_base <req> <root>: no git here. Sets base, base_why, base_mb ("-" base = no base column).
#   --base <full sha> (explicit, wins), else the role's .synced `merge_base=` (the fork point of the synced HEAD and the sync's
#   --base ref: "what this branch changed" is measured against it), else none. `--base none` drops the column.
#   .synced also holds `base_sha=` (the tip of the sync's --base ref); it is NOT the default here (a branch behind its base
#   would count the base's newer commits as its own) but `--base <that sha>` names it. base_mb = .synced merge_base ("-" if none).
#   The base's SOURCE or PRODUCTS must already be on this host (~/wl/src/<sha>, ~/wl/products/<sha>; built and cached by
#   wl_prep.sh like any rev). Without git here they cannot be fetched: prime them once from a git checkout with
#   `bash validation/wl_prime_base.sh <host> <sha>` (ships `git archive <sha>`; no git on the host needed).
wl_local_base() {
    local req=$1 root=$2 mb tip
    mb=$(wl_synced_get "$root" merge_base); tip=$(wl_synced_get "$root" base_sha)
    base_mb=${mb:--}
    case $req in
        none) base=-; base_why="--base none: no base column"; return 0 ;;
        "")
            if [ -z "$mb" ]; then base=-; base_why="no --base and no merge_base in .synced (sync with --base): no base column"; return 0; fi
            req=$mb; base_why="default: .synced merge_base of HEAD and $(wl_synced_get "$root" base) (base_sha=${tip:0:9})" ;;
        *[!0-9a-f]*) echo "wl: --base needs a full commit sha here (no git in a role directory; .synced has merge_base=${mb:-none} base_sha=${tip:-none}); got '$req'" >&2; return 2 ;;
        *) base_why="--base $req as given"; [ "$req" = "$tip" ] && base_why="$base_why (= .synced base_sha)" ;;
    esac
    [ ${#req} = 40 ] || { echo "wl: base '$req' is not a full 40-hex sha" >&2; return 2; }
    if [ -d "$HOME/wl/src/$req" ] || [ -f "$HOME/wl/products/$req/libc.a" ]; then base=$req; return 0; fi
    echo "wl: base $req is not on this host (~/wl/src/$req or ~/wl/products/$req missing); prime it from a git checkout: bash validation/wl_prime_base.sh $(hostname | cut -d. -f1) $req (or --base none)" >&2
    return 2
}

# wl_install_local: B = $HOME/wl/bin/v-<hash of wl_* >, a private copy of the scripts (a later sync of the role cannot change
# a running job). Same naming as wl_install_scripts.
wl_install_local() {
    local h inc
    h=$(cat "$here"/wl_* | md5sum | cut -c1-12); B="$HOME/wl/bin/v-$h"
    mkdir -p "$HOME/wl/bin"; find "$HOME/wl/bin" -maxdepth 1 -name 'v-*' -mtime +2 -exec rm -rf {} + 2>/dev/null
    [ -d "$B" ] && return 0
    inc=$HOME/wl/bin/.in.$$; rm -rf "$inc"; mkdir -p "$inc" && cp "$here"/wl_* "$inc"/ && { mv -T "$inc" "$B" 2>/dev/null || rm -rf "$inc"; }
    [ -d "$B" ]
}

# wl_local_run <label> <cmd> : run $cmd under bash -l (cargo on PATH) from a script file (no quoting trouble); output to
# ~/logs/<label>.log (and the caller's stdout), exit status to ~/logs/<label>.done. Returns the command's status.
wl_local_run() {
    local label=$1 cmd=$2 rc
    mkdir -p "$HOME/logs"
    printf '%s\n' "$cmd" > "$HOME/logs/$label.sh"
    bash -l "$HOME/logs/$label.sh" > "$HOME/logs/$label.log" 2>&1 < /dev/null; rc=$?
    echo "$rc" > "$HOME/logs/$label.done"
    return "$rc"
}
