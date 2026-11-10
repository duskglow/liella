#!/bin/sh
# WCTYPE-TRIE-FILE counts job: products, wcprobe counts (Liella, glibc), first-touch instructions, cbench_locale.
# usage: count.sh <role dir> <tag>   (outputs in ~/work/WCTYPE-TRIE-FILE/<tag>/)
set -u
D=$(cd "$(dirname "$0")" && pwd)
R=$1
W=$HOME/work/WCTYPE-TRIE-FILE/$2
mkdir -p "$W"
. "$HOME/.cargo/env"
cd "$R" || exit 2
echo "tree $(cat "$R/.synced" 2>/dev/null | tr '\n' ' ')"
cargo xtask so > "$W/so.log" 2>&1 || { tail -20 "$W/so.log"; exit 3; }
INC=$R/include
[ -d "$INC" ] || INC=$R/csys/include
L=$R/target/liella
cd "$W" || exit 2
for p in wcprobe:$R/handoff/wctype-rethink/wcprobe.c touch:$D/touch.c; do
    n=${p%%:*}; s=${p#*:}
    gcc -std=gnu17 -O2 -fno-builtin -static -nostdlib -DLIELLA -I "$INC" "$L/liella-crt1.o" "$s" "$L/libc.a" -lgcc -o "$n-liella" >> build.log 2>&1 || { head -40 build.log; exit 4; }
    gcc -std=gnu17 -O2 -fno-builtin -fno-plt -Wl,-z,now "$s" -o "$n-glibc" >> build.log 2>&1 || { head -40 build.log; exit 5; }
done
./wcprobe-liella count count-liella.tsv > count-liella.log 2>&1
echo "count liella rc $?"
./wcprobe-glibc count count-glibc.tsv > count-glibc.log 2>&1
echo "count glibc rc $?"
for impl in liella glibc; do
    for loc in en_US.UTF-8 C.UTF-8; do
        for m in all one; do
            v=$(perf stat -x, -e instructions:u "./touch-$impl" "$loc" "$m" 2>&1 >/dev/null | awk -F, '/instructions/ {print $1}')
            echo "touch $impl $loc $m $v"
        done
    done
done
cd "$R" || exit 2
cargo xtask bench --compare --counters cbench_locale > "$W/bench.log" 2>&1
echo "bench rc $?"
grep -E "isw|tow" "$W/bench.log" | head -40
