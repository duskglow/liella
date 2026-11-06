#!/bin/sh
# Name-service cache attach cost on a build host (no spec clause: measurement only, D-16.10).
# usage: measure.sh <role dir> <tag> [<work dir>]
# Builds percall.c statically against the role's target/liella (percall-<tag>) and steps.c against the host libc,
# then prints per-call kernel/user instructions and faults with the cache on and off, an strace of the second call,
# and the kernel cost of each attach system call alone (steps). The region is /dev/shm/liella-nscache-<uid>.
set -u
R=${1:?role dir}
T=${2:?tag}
W=${3:-$PWD}
P=$(cd "$(dirname "$0")" && pwd)
cd "$W" || exit 2
L=$R/target/liella
INC=$R/include
[ -d "$INC" ] || INC=$R/csys/include
gcc -std=gnu17 -O1 -fno-builtin -static -nostdlib -I "$INC" "$L/liella-crt1.o" "$P/percall.c" "$L/libc.a" -lgcc -o "percall-$T" || exit 3
gcc -O1 -o steps "$P/steps.c" || exit 4
U=$(id -un)
REG=/dev/shm/liella-nscache-$(id -u)
for mode in 1 1 1 0 0; do
  echo "-- LIELLA_NSCACHE=$mode"
  env LIELLA_NSCACHE=$mode "./percall-$T" 4 "$U"
done
echo "-- re-attach with warm code (setuid before call 3)"
env LIELLA_NSCACHE=1 "./percall-$T" 4 "$U" x
echo "== strace of a cached process (call 2)"
env LIELLA_NSCACHE=1 strace -T "./percall-$T" 2 "$U" 2>&1 | grep -v '^call' | awk '/getppid/{n++} n>=3 && n<5' | sed -n '1,60p'
echo "== attach steps alone"
./steps "$REG"
