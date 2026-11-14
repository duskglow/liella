#!/bin/sh
# run.sh - DL-18 (DL-DLADDR) black-box probe of dladdr, dladdr1 and dlinfo (spec/behaviour/dynlink.md
# "dladdr and dlinfo"). Builds lib.c and probe.c in SCRATCH (created, must not exist), runs the probe
# and prints the record on stdout with SCRATCH shown as <dir>. Public interfaces only: the reference
# loader is executed; the objects' files are read for section headers and .dynsym order.
#   sh tools/probe/dl/dladdr/run.sh SCRATCH
set -u
SRC=$(cd "$(dirname "$0")" && pwd)
W=${1:?usage: run.sh SCRATCH}
[ -e "$W" ] && { echo "run.sh: $W exists" >&2; exit 2; }
mkdir -p "$W/sub" || exit 2
W=$(cd "$W" && pwd)
CC=${CC:-cc}
export LC_ALL=C
unset LD_LIBRARY_PATH LD_PRELOAD LD_BIND_NOW GLIBC_TUNABLES 2>/dev/null
$CC -O1 -fPIC -shared -o "$W/libP.so" "$SRC/lib.c" || exit 2
$CC -O1 -fPIC -shared -DNOTLS -o "$W/sub/libQ.so" "$SRC/lib.c" \
    -Wl,--enable-new-dtags,-rpath,'/r1:$ORIGIN/x:/r2' || exit 2
$CC -O1 -fPIC -shared -DNOTLS -o "$W/sub/libR.so" "$SRC/lib.c" -Wl,--disable-new-dtags,-rpath,/rp || exit 2
$CC -O1 -o "$W/probe" "$SRC/probe.c" -Wl,-rpath,/prog/rp -pthread || exit 2
$CC -O1 -rdynamic -o "$W/probe_rd" "$SRC/probe.c" -pthread || exit 2
echo "== probe (no -rdynamic, DT_RPATH /prog/rp)"
(cd "$W" && timeout 60 ./probe "$W" "$W/libP.so" "$W/sub/libQ.so" "$W/sub/libR.so" ./sub/libQ.so)
echo "== probe_rd (-rdynamic), LD_LIBRARY_PATH=/l1:/l2"
(cd "$W" && LD_LIBRARY_PATH=/l1:/l2 timeout 60 ./probe_rd "$W" "$W/libP.so" | grep -E "program|probe_exported|main|SERINFO|\[[0-9]+\]")
