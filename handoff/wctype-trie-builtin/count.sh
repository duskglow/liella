#!/bin/sh
# WCTYPE-TRIE-BUILTIN counts job: products, wcprobe (Liella and glibc) counts, cbench_locale counters.
# usage: count.sh <tag>   (outputs in ~/work/WCTYPE-TRIE-BUILTIN/<tag>/)
set -u
TAG=$1
R=$HOME/w/n1
SRC=$HOME/work/WCTYPE-TRIE-BUILTIN
W=$SRC/$TAG
mkdir -p "$W"
. "$HOME/.cargo/env"
cd "$R" || exit 2
echo "tree $(cat "$R/.synced" 2>/dev/null | tr '\n' ' ')"
cargo xtask so > "$W/so.log" 2>&1 || { tail -20 "$W/so.log"; exit 3; }
INC=$R/include
[ -d "$INC" ] || INC=$R/csys/include
L=$R/target/liella
cd "$W" || exit 2
gcc -std=gnu17 -O2 -fno-builtin -static -nostdlib -DLIELLA -I "$INC" "$L/liella-crt1.o" "$R/handoff/wctype-rethink/wcprobe.c" "$L/libc.a" -lgcc -o wc-liella > build.log 2>&1 || { head -40 build.log; exit 4; }
gcc -std=gnu17 -O2 -fno-builtin -fno-plt -Wl,-z,now "$R/handoff/wctype-rethink/wcprobe.c" -o wc-glibc >> build.log 2>&1 || { head -40 build.log; exit 5; }
./wc-liella count count-liella.tsv > count-liella.log 2>&1
echo "count liella rc $?"
./wc-glibc count count-glibc.tsv > count-glibc.log 2>&1
echo "count glibc rc $?"
cd "$R" || exit 2
cargo xtask bench --compare --counters cbench_locale > "$W/bench.log" 2>&1
echo "bench rc $?"
tail -60 "$W/bench.log"
