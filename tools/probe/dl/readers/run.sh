#!/bin/sh
# run.sh - DL-16c black-box probe of the readers in a dynamically linked program (R-1..R-4, see
# probe.c): _dl_find_object's range and link map, dl_iterate_phdr names and counters, and a
# callback that calls dlopen. Builds in SCRATCH (created, must not exist) and prints the record.
# Public interfaces only; the reference's loader and libraries are only executed.
#   sh tools/probe/dl/readers/run.sh SCRATCH
set -u
SRC=$(cd "$(dirname "$0")" && pwd)
W=${1:?usage: run.sh SCRATCH}
[ -e "$W" ] && { echo "run.sh: $W exists" >&2; exit 2; }
mkdir -p "$W" || exit 2
W=$(cd "$W" && pwd)
CC=${CC:-cc}
export LC_ALL=C
unset LD_LIBRARY_PATH LD_PRELOAD LD_BIND_NOW GLIBC_TUNABLES 2>/dev/null
for n in R1 R2; do
    $CC -std=gnu17 -O1 -fPIC -shared -DNAME="$n" -o "$W/lib$n.so" "$SRC/lib.c" || exit 2
done
$CC -std=gnu17 -O1 -o "$W/probe" "$SRC/probe.c" || exit 2
echo "== readers (DL-16c)"
timeout 60 "$W/probe" "$W" 2>&1 | sed "s|$W|<dir>|g"
