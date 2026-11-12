#!/bin/bash
# LAB-ONLY: this script drives the project's own test hosts and is not needed to build or test Liella.
# Self-test of the products profile and cache key of validation/wl_lock.sh (WL-PREP-RELEASE-PROFILE; no spec clause:
# measurement tooling). The workload gate and the per-call counts must measure what ships, the release profile:
#   1. wl_key: <sha>-release by default, <sha> (the old key) with WL_PROFILE=fast, an unknown profile refused;
#   2. wl_build_products builds with LIELLA_FULL_PROFILE=1 and refuses a build that left fast products;
#   3. a fast set cached under the old key <sha> (no .profile marker) is never taken for release, and is kept;
#   4. a cached set of the other profile under a key is refused, never reused, rebuilt over or deleted;
#   5. WL_PROFILE=fast reads the old fast cache without building.
# Runs the real wl_lock.sh functions in a scratch HOME with a stand-in `cargo` that writes products and xtask's
# target/liella/.profile marker the way `cargo xtask so` does (fast unless LIELLA_FULL_PROFILE is set and not 0).
# Seconds; no Liella build. `affected` runs it when wl_lock.sh or this file changes (VALIDATION_COVERED).
set -u
here=$(cd "$(dirname "$0")" && pwd)
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
export HOME=$tmp/home
unset LIELLA_FULL_PROFILE WL_PROFILE
stub=$tmp/stub
mkdir -p "$HOME/wl/src" "$HOME/wl/products" "$stub"
# bash -l (wl_build_products) may reset PATH from /etc/profile; the scratch HOME's .profile puts the stand-in first again.
printf 'PATH=%s:$PATH\n' "$stub" > "$HOME/.profile"
export PATH=$stub:$PATH
printf '%s\n' '#!/bin/sh' \
    '[ "$1 $2" = "xtask so" ] || { echo "stand-in cargo: unexpected $*" >&2; exit 2; }' \
    'echo build >> "$HOME/builds"' \
    'p=fast; case ${LIELLA_FULL_PROFILE:-} in ""|0) ;; *) p=release ;; esac; [ -n "${STUB_PROFILE:-}" ] && p=$STUB_PROFILE' \
    'mkdir -p target/liella && for f in libc.a libm.a liella-crt1.o; do echo "$p" > target/liella/$f; done' \
    'echo "$p" > target/liella/.profile' > "$stub/cargo"
chmod +x "$stub/cargo"
. "$here/wl_lock.sh"
pass=0; fail=0
check() { if [ "$2" = "$3" ]; then pass=$((pass + 1)); else fail=$((fail + 1)); echo "FAIL $1: expected [$2] got [$3]"; fi; }
builds() { if [ -f "$HOME/builds" ]; then wc -l < "$HOME/builds" | tr -d ' '; else echo 0; fi; }
S=0123456789abcdef0123456789abcdef01234567
T=89abcdef0123456789abcdef0123456789abcdef
U=fedcba9876543210fedcba9876543210fedcba98
for s in $S $T $U; do mkdir -p "$HOME/wl/src/$s"; done

# 1. keys
check "default key" "$S-release" "$(wl_key $S)"
check "fast key" "$S" "$(WL_PROFILE=fast wl_key $S)"
check "release key" "$S-release" "$(WL_PROFILE=release wl_key $S)"
check "glibc variant key" glibc "$(wl_vkey glibc)"
check "commit variant key" "$S-release" "$(wl_vkey $S)"
WL_PROFILE=debug wl_key $S > /dev/null 2>&1; check "unknown profile refused (rc)" 2 "$?"

# 2 + 3. a legacy fast set under <sha> (cached before this change: no marker), then a release build of that sha
mkdir -p "$HOME/wl/products/$S" && echo legacy > "$HOME/wl/products/$S/libc.a" && echo legacy > "$HOME/wl/products/$S/libm.a"
wl_products_ok "$HOME/wl/products/$S" 2> /dev/null; check "legacy set is fast, not release (rc)" 2 "$?"
wl_build_products $S > "$tmp/b1.log" 2>&1; check "release build of a sha with a legacy fast cache (rc)" 0 "$?"
check "release build ran cargo once" 1 "$(builds)"
check "release products marker" release "$(cat "$HOME/wl/products/$S-release/.profile" 2>/dev/null)"
check "release libc.a built by the release profile" release "$(cat "$HOME/wl/products/$S-release/libc.a" 2>/dev/null)"
check "legacy fast cache kept" legacy "$(cat "$HOME/wl/products/$S/libc.a" 2>/dev/null)"
wl_build_products $S > /dev/null 2>&1; check "second release build reuses the cache (rc)" 0 "$?"
check "second release build ran no cargo" 1 "$(builds)"

# 5. the fast profile reads the legacy cache without building
WL_PROFILE=fast wl_build_products $S > /dev/null 2>&1; check "fast build reuses the legacy cache (rc)" 0 "$?"
check "fast reuse ran no cargo" 1 "$(builds)"
WL_PROFILE=fast wl_build_products $U > /dev/null 2>&1; check "fast build of a new sha (rc)" 0 "$?"
check "fast products marker" fast "$(cat "$HOME/wl/products/$U/.profile" 2>/dev/null)"
check "release key untouched by a fast build" no "$([ -e "$HOME/wl/products/$U-release" ] && echo yes || echo no)"

# 2b. a build that leaves products of the other profile (STUB_PROFILE: the stand-in ignores the switch) is refused
V=00112233445566778899aabbccddeeff00112233
mkdir -p "$HOME/wl/src/$V"
STUB_PROFILE=fast wl_build_products $V > "$tmp/b2.log" 2>&1; check "build that left fast products refused (rc)" 1 "$?"
check "refused build cached nothing" no "$([ -e "$HOME/wl/products/$V-release" ] && echo yes || echo no)"

# 4. the other profile under a key: refused, kept, not rebuilt
mkdir -p "$HOME/wl/products/$T-release" && for f in libc.a libm.a; do echo fast > "$HOME/wl/products/$T-release/$f"; done
echo fast > "$HOME/wl/products/$T-release/.profile"
n=$(builds)
wl_build_products $T > "$tmp/b4.log" 2>&1; check "fast set under a release key refused (rc)" 1 "$?"
check "refused set kept" fast "$(cat "$HOME/wl/products/$T-release/libc.a" 2>/dev/null)"
check "refused set not rebuilt" "$n" "$(builds)"
check "refusal names the profiles" 1 "$(grep -c "holds products of profile 'fast', this run wants release" "$tmp/b4.log")"

echo "wl_lock_test: $pass passed, $fail failed"
[ $fail = 0 ]
