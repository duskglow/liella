# LAB-ONLY: this script drives the project's own test hosts and is not needed to build or test Liella.
# Sourced by wl_prep.sh and wl_percall_run.sh (copied to the Zen 1 host (~/wl/bin) with them), and for wl_key by every
# script that reads ~/wl/products, ~/wl/kit or the per-sha row caches. Self-test: validation/wl_lock_test.sh.
# BUILD LOCK (TOOLS-SYNC, 2026-10-10): ~/wl/src/<sha> and ~/wl/products/<key> are shared by every wl_count/wl_percall/
# wl_gate run; two concurrent `cargo xtask so` in one tree crashed `ar` with SIGBUS. One flock per sha serialises the
# build and the kit links; the second caller waits, then finds the products. Products appear atomically (tmp + mv).
# PROFILE (WL-PREP-RELEASE-PROFILE): the products are the SHIPPED build, `cargo xtask so` with LIELLA_FULL_PROFILE=1
# (xtask's own switch to the release profile), never the fast test profile, whose code differs from what ships (it
# reads some statics through the GOT: +1 instruction per read). WL_PROFILE=fast (only for a comparison) builds the
# fast profile. Everything made from products is keyed by `wl_key <sha>`: <sha>-release for release, <sha> for fast.
# <sha> was the only key before this change and every product cached under it is fast, so those caches stay readable
# as fast and are never taken for release. Each products directory keeps xtask's `.profile` marker (target/liella/
# .profile); wl_products_ok checks it (no marker: a fast set cached before the marker was kept).
wl_lock() {   # wl_lock <name>: take ~/wl/locks/<name>.lock on fd 8 (released when the shell exits or on wl_unlock)
    mkdir -p "$HOME/wl/locks"; exec 8> "$HOME/wl/locks/$1.lock"
    flock -n 8 || { echo "wl: waiting for another run building $1" >&2; flock -w 5400 8 || { echo "wl: lock $1 not free after 90 min" >&2; exit 1; }; }
}
wl_unlock() { flock -u 8; exec 8>&-; }
wl_profile() {   # the products profile of this run: release (default) or fast (WL_PROFILE=fast)
    case ${WL_PROFILE:-release} in
        release|fast) echo "${WL_PROFILE:-release}" ;;
        *) echo "wl: WL_PROFILE=${WL_PROFILE}: want release or fast" >&2; return 2 ;;
    esac
}
wl_key() {   # wl_key <sha>: the cache key of <sha>'s products, kit, per-call binaries and count rows in this profile
    local p
    p=$(wl_profile) || return 2
    if [ "$p" = release ]; then echo "$1-release"; else echo "$1"; fi
}
wl_vkey() {   # wl_vkey <variant>: glibc stays glibc (the host build, no products); a commit -> wl_key
    if [ "$1" = glibc ]; then echo glibc; else wl_key "$1"; fi
}
wl_products_ok() {   # wl_products_ok <dir>: 0 = a complete set of this run's profile; 1 = none (or incomplete); 2 = the other profile
    local d=$1 p m
    p=$(wl_profile) || return 2
    # libm.a too (GAWK-LM-CHECK-2): gawk links it in place of the old maths stand-in.
    [ -f "$d/libc.a" ] && [ -f "$d/libm.a" ] || return 1
    m=$(cat "$d/.profile" 2>/dev/null) || m=fast
    [ "$m" = "$p" ] && return 0
    echo "wl: $d holds products of profile '$m', this run wants $p (keys: wl_key in validation/wl_lock.sh)" >&2
    return 2
}
wl_build_products() {   # wl_build_products <sha>: products of <sha> in ~/wl/products/$(wl_key <sha>) (call with the lock held)
    local sha=$1 p key prod full m rc t=$HOME/wl/src/$1/target/liella
    p=$(wl_profile) && key=$(wl_key "$sha") || return 2
    prod=$HOME/wl/products/$key
    wl_products_ok "$prod"; rc=$?
    [ $rc = 0 ] && return 0
    [ $rc = 1 ] || return 1   # the other profile under this key: never reused, rebuilt over or deleted
    full=; [ "$p" = release ] && full=1
    (cd "$HOME/wl/src/$sha" && LIELLA_FULL_PROFILE=$full bash -lc 'cargo xtask so' > "$HOME/wl/src/$key.build.log" 2>&1) || { tail -20 "$HOME/wl/src/$key.build.log"; return 1; }
    m=$(cat "$t/.profile" 2>/dev/null)
    [ "$m" = "$p" ] || { echo "wl: $sha: the build left products of profile '$m' in $t, this run wants $p" >&2; return 1; }
    rm -rf "$prod.tmp" && mkdir -p "$prod.tmp" && cp "$t/libc.a" "$t/libm.a" "$t/liella-crt1.o" "$t/.profile" "$prod.tmp/" && rm -rf "$prod" && mv -T "$prod.tmp" "$prod"
}
