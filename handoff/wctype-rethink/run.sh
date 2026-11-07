#!/bin/sh
# WCTYPE-RETHINK measurement job (build host): products, probe built both ways, stats/verify, counts.
set -u
R=$HOME/w/s4
W=$HOME/work/WCTYPE-RETHINK
. "$HOME/.cargo/env"
cd "$R" || exit 2
cargo xtask so > "$W/so.log" 2>&1 || { tail -20 "$W/so.log"; exit 3; }
cd "$W" || exit 2
INC=$R/include
[ -d "$INC" ] || INC=$R/csys/include
L=$R/target/liella
gcc -std=gnu17 -O2 -fno-builtin -static -nostdlib -DLIELLA -DPROTO -I "$INC" "$L/liella-crt1.o" wcprobe.c "$L/libc.a" -lgcc -o wc-liella > build.log 2>&1 || { head -40 build.log; exit 4; }
gcc -std=gnu17 -O2 -fno-builtin -fno-plt -Wl,-z,now wcprobe.c -o wc-glibc >> build.log 2>&1 || { head -40 build.log; exit 5; }
gcc -std=gnu17 -O2 -fno-builtin -static -nostdlib -DLIELLA -I "$INC" "$L/liella-crt1.o" wcprobe.c "$L/libc.a" -lgcc -o wc-liella-noproto >> build.log 2>&1 || { head -40 build.log; exit 6; }
echo "built from $(cat "$R/.synced" | tr '\n' ' ')"
./wc-liella stats > stats.txt 2>&1
echo "stats rc $?"
./wc-liella count count-liella.tsv > count-liella.log 2>&1
echo "count liella rc $?"
./wc-glibc count count-glibc.tsv > count-glibc.log 2>&1
echo "count glibc rc $?"
./wc-liella-noproto count count-liella-noproto.tsv > count-liella-noproto.log 2>&1
echo "count liella-noproto rc $?"
tar czf wc-bins.tgz wc-liella wc-glibc trun.sh
