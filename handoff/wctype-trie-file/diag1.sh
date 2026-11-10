#!/bin/sh
# WCTYPE-TRIE-FILE-5 diag1: the first localtime_r's page faults on this head, by address.
set -e
. ~/.cargo/env
W=~/work/WCTYPE-TRIE-FILE-5
cd ~/w/sh1
cargo xtask so
cc -std=gnu17 -O1 -static -nostdlib -fno-builtin target/liella/liella-crt1.o $W/pfmark.c target/liella/libc.a -lgcc -o $W/pfmark
nm -n -S $W/pfmark > $W/pfmark.nm
readelf -SW $W/pfmark > $W/pfmark.sec
cd $W
for i in 1 2 3; do env -i LANG=en_US.UTF-8 setarch -R ./pfmark; done
env -i LANG=en_US.UTF-8 setarch -R perf record -q -e page-faults -c 1 -d -o $W/pf.data ./pfmark 2> $W/run.err
cat $W/run.err
perf script -i $W/pf.data -F addr,ip > $W/pf.txt
A=$(sed -n 's/.* a=0x\([0-9a-f]*\) .*/\1/p' $W/run.err)
B=$(sed -n 's/.* b=0x\([0-9a-f]*\).*/\1/p' $W/run.err)
python3 $W/resolve.py $W/pfmark.nm $W/pfmark.sec $W/pf.txt $A $B
readelf -lW $W/pfmark | grep -E 'LOAD|TLS'
