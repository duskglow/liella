#!/bin/sh
# run.sh - DL-DLOPEN-LOAD-2 black-box probe of the dlopen spec gaps (ruling #5266): texts for a long
# non-ELF file, other malformed headers, no PT_DYNAMIC, EI_CLASS other than 1/2, and whether an
# RTLD_NOLOAD miss clears a pending dlerror text. Builds lib.c and probe.c in SCRATCH (created, must not
# exist) and prints the record (the `== gaps` part of spec/behaviour/dynlink.glibc.txt).
#   sh tools/probe/dl/gaps/run.sh SCRATCH
# Public interfaces only; the reference's loader is only executed. Each case runs in a forked child.
set -u
SRC=$(cd "$(dirname "$0")" && pwd)
W=${1:?usage: run.sh SCRATCH}
[ -e "$W" ] && { echo "run.sh: $W exists" >&2; exit 2; }
mkdir -p "$W" || exit 2
W=$(cd "$W" && pwd)
CC=${CC:-cc}
export LC_ALL=C
unset LD_LIBRARY_PATH LD_PRELOAD LD_BIND_NOW GLIBC_TUNABLES 2>/dev/null
$CC -std=gnu17 -O1 -fPIC -shared -o "$W/libok.so" "$SRC/lib.c" || exit 2
$CC -std=gnu17 -O1 -o "$W/probe" "$SRC/probe.c" -ldl || exit 2
timeout 120 "$W/probe" "$W"
