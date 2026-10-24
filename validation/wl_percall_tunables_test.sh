#!/bin/bash
# LAB-ONLY: this script drives the project's own test hosts and is not needed to build or test Liella.
# Check for TOOL-PERCALL-TUNABLES-KEY (no spec clause: measurement tooling): two wl_percall.sh runs of one AVX2-dispatched
# row (strlen_4k) started TOGETHER, one with GLIBC_TUNABLES=glibc.cpu.hwcaps=-AVX2 and one default, must
#   1. print their own tunables value in the header,
#   2. use different row files (the key is part of the cache file name),
#   3. report different instruction counts for Liella (the dispatch level changes the code that runs).
# It runs the real tool on the host where it is started (keke); COUNT_FRESH is not set on purpose: the second pair of
# runs after the first must hit each run's OWN cached row and print the same numbers.
#   ~/bin/jobrun <label> count -- sh -c 'cd ~/w/<role> && bash validation/wl_percall_tunables_test.sh .'
set -u
tree=${1:-.}
here=$(cd "$(dirname "$0")" && pwd)
out=$(mktemp -d)
trap 'rm -rf "$out"' EXIT
spec=$here/percall/strstr_getline.c
run() { # name tunables
    ( if [ -n "$2" ]; then export GLIBC_TUNABLES=$2; else unset GLIBC_TUNABLES; fi
      WLP_TAIL=0 bash "$here/wl_percall.sh" --base none --spec "$spec" --cases strlen_4k "$tree" > "$out/$1.log" 2>&1; echo $? > "$out/$1.rc" )
}
pass=0; fail=0
check() { if [ "$2" = "$3" ]; then pass=$((pass + 1)); else fail=$((fail + 1)); echo "FAIL $1: expected [$2] got [$3]"; fi; }
ne() { if [ "$2" != "$3" ] && [ -n "$2" ] && [ -n "$3" ]; then pass=$((pass + 1)); else fail=$((fail + 1)); echo "FAIL $1: [$2] vs [$3] must differ and be non-empty"; fi; }
col() { awk '$1 == "strlen_4k" { print $'"$2"'; exit }' "$out/$1.log"; }   # 2 = glibc column, 4 = new (no base column value)
run sse2 glibc.cpu.hwcaps=-AVX2 &
run dflt "" &
wait
check "sse2 rc" 0 "$(cat "$out/sse2.rc")"
check "default rc" 0 "$(cat "$out/dflt.rc")"
check "sse2 header" "1" "$(grep -c 'env key: GLIBC_TUNABLES=glibc.cpu.hwcaps=-AVX2;' "$out/sse2.log")"
check "default header" "1" "$(grep -c 'env key: GLIBC_TUNABLES=<unset>;' "$out/dflt.log")"
k1=$(grep -o 'row cache key [^;]*' "$out/sse2.log"); k2=$(grep -o 'row cache key [^;]*' "$out/dflt.log")
ne "cache keys" "$k1" "$k2"
ne "Liella strlen_4k instructions/call, SSE2 vs default" "$(col sse2 4)" "$(col dflt 4)"
# second pair: served from each run's own cache, same numbers
run sse2b glibc.cpu.hwcaps=-AVX2 &
run dfltb "" &
wait
check "sse2 cached" "$(col sse2 4)" "$(col sse2b 4)"
check "default cached" "$(col dflt 4)" "$(col dfltb 4)"
for f in sse2 dflt sse2b dfltb; do echo "== $f: $(grep -E '^(env key|strlen_4k)' "$out/$f.log" | tr '\n' '|')"; done
echo "wl_percall_tunables_test: $pass passed, $fail failed"
[ $fail = 0 ]
