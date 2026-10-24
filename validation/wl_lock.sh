# LAB-ONLY: this script drives the project's own test hosts and is not needed to build or test Liella.
# Sourced by wl_prep.sh and wl_percall_run.sh (copied to keke:~/wl/bin with them).
# BUILD LOCK (TOOLS-SYNC, 2026-10-10): ~/wl/src/<sha> and ~/wl/products/<sha> are shared by every wl_count/wl_percall/
# wl_gate run; two concurrent `cargo xtask so` in one tree crashed `ar` with SIGBUS. One flock per sha serialises the
# build and the kit links; the second caller waits, then finds the products. Products appear atomically (tmp + mv).
wl_lock() {   # wl_lock <name>: take ~/wl/locks/<name>.lock on fd 8 (released when the shell exits or on wl_unlock)
    mkdir -p "$HOME/wl/locks"; exec 8> "$HOME/wl/locks/$1.lock"
    flock -n 8 || { echo "wl: waiting for another run building $1" >&2; flock -w 5400 8 || { echo "wl: lock $1 not free after 90 min" >&2; exit 1; }; }
}
wl_unlock() { flock -u 8; exec 8>&-; }
wl_build_products() {   # wl_build_products <sha>: products of <sha> in ~/wl/products/<sha> (call with the lock held)
    local sha=$1 prod=$HOME/wl/products/$1
    # libm.a too (GAWK-LM-CHECK-2): gawk links it in place of the old maths stand-in.
    [ -f "$prod/libc.a" ] && [ -f "$prod/libm.a" ] && return 0
    (cd "$HOME/wl/src/$sha" && bash -lc 'cargo xtask so' > "$HOME/wl/src/$sha.build.log" 2>&1) || { tail -20 "$HOME/wl/src/$sha.build.log"; return 1; }
    rm -rf "$prod.tmp" && mkdir -p "$prod.tmp" && cp "$HOME/wl/src/$sha/target/liella/libc.a" "$HOME/wl/src/$sha/target/liella/libm.a" "$HOME/wl/src/$sha/target/liella/liella-crt1.o" "$prod.tmp/" && rm -rf "$prod" && mv -T "$prod.tmp" "$prod"
}
