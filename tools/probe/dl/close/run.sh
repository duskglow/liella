#!/bin/sh
# run.sh - DL-17 black-box probe of dlclose (C-1..C-9, see probe.c). Builds in SCRATCH (created, must
# not exist) and prints the record. Public interfaces only; the reference's loader and libraries are
# only executed.
#   sh tools/probe/dl/close/run.sh SCRATCH
set -u
SRC=$(cd "$(dirname "$0")" && pwd)
W=${1:?usage: run.sh SCRATCH}
[ -e "$W" ] && { echo "run.sh: $W exists" >&2; exit 2; }
mkdir -p "$W" || exit 2
W=$(cd "$W" && pwd)
CC=${CC:-cc}
export LC_ALL=C
unset LD_LIBRARY_PATH LD_PRELOAD LD_BIND_NOW GLIBC_TUNABLES 2>/dev/null
lib() { n=$1; shift; $CC -std=gnu17 -O1 -fPIC -shared -DNAME="$n" "$@" -o "$W/lib$n.so" "$SRC/lib.c" || exit 2; }
lib P
lib G -DGDEF
lib U -DGUSE
lib TG -DTLS_GD
lib TI -DTLS_IE
lib Q -DUNIQ
lib N
lib Y
lib X -DOPENER -DYPATH="$W/libY.so"
$CC -std=gnu17 -O1 -o "$W/probe" "$SRC/probe.c" || exit 2
echo "== close (DL-17)"
timeout 60 "$W/probe" "$W" 2>&1 | sed "s|$W|<dir>|g"
