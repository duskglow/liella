#!/bin/sh
# run.sh - DL-16P black-box probes P-1..P-13 of plan/ph14/dl-16-dlopen.md section 6 (dlopen modes,
# identity, RTLD_GLOBAL, scopes, RTLD_NEXT, dlerror, recursive dlopen, readers during a constructor,
# fork during dlopen, IFUNC on a failed dlopen, surplus TLS in older threads, dlinfo link maps, dlopen
# of the program / ld.so / libc). Builds the corpus from lib.c and probe.c in SCRATCH (created, must
# not exist), runs every probe and prints the record on stdout with SCRATCH shown as <dir>.
# Public interfaces only; the reference's loader and libraries are only executed.
#   sh tools/probe/dl/dlopen/run.sh SCRATCH    (its output is the `== P-n` part of
#                                               spec/behaviour/dynlink.glibc.txt)
# Hang guards: every case runs in a forked child with alarm(30); P-8 and P-9 record "blocked" after
# 5 s and then release the waiting constructor; every probe program runs under timeout(1).
set -u
SRC=$(cd "$(dirname "$0")" && pwd)
W=${1:?usage: run.sh SCRATCH}
[ -e "$W" ] && { echo "run.sh: $W exists" >&2; exit 2; }
mkdir -p "$W/id" "$W/id2" || exit 2
W=$(cd "$W" && pwd)
CC=${CC:-cc}
CF="-std=gnu17 -O1 -fPIC -w"
export LC_ALL=C
unset LD_LIBRARY_PATH LD_PRELOAD LD_BIND_NOW GLIBC_TUNABLES 2>/dev/null

lib() { # lib NAME OUT [extra cc args...]
    lib_n=$1; lib_o=$2; shift 2
    $CC $CF -shared -DNAME="$lib_n" -o "$lib_o" "$SRC/lib.c" -Wl,--no-as-needed "$@" || { echo "build failed: $lib_o" >&2; exit 2; }
}
R="-L$W -Wl,-rpath,$W"

# ---- corpus ----
lib C1 "$W/libC1.so"
lib C1b "$W/libC1b.so"
lib U1 "$W/libU1.so" -DUNDEF
lib Id "$W/id/libId.so.1" -Wl,-soname,libId.so.1
ln -s libId.so.1 "$W/id/sym.so"
ln "$W/id/libId.so.1" "$W/id/hard.so"
cp "$W/id/libId.so.1" "$W/id2/libId.so.1"
lib NeedId "$W/libNeedId.so" -DUSE=Id -L"$W/id2" -l:libId.so.1 -Wl,--enable-new-dtags,-rpath,"$W/id2"
lib B3 "$W/libB3.so"
lib A3 "$W/libA3.so" $R -lB3
lib Z3 "$W/libZ3.so" -DUSE=A3 -DUSE2=B3
lib C4 "$W/libC4.so"
lib B4 "$W/libB4.so" -DUSE=C4 -DUSE2=A4
lib A4 "$W/libA4.so" $R -lB4 -lC4
lib M5 "$W/libM5.so" -DWHO
lib L5 "$W/libL5.so" -DWHO -DLOOKUP $R -lM5
lib G5 "$W/libG5.so" -DWHO -DLOOKUP
lib A7 "$W/libA7.so" -DP7
lib B7 "$W/libB7.so" -DCALL=A7 $R -lA7
lib A8 "$W/libA8.so" -DWAITCTOR
lib A9 "$W/libA9.so" -DWAITCTOR
lib I10 "$W/libI10.so" -DIFUNC
lib U10 "$W/libU10.so" -DUNDEF
lib R10a "$W/libR10a.so" -DUSEIFN $R -lI10 -lU10
lib R10b "$W/libR10b.so" -DUSEIFN $R -lU10 -lI10
lib R10c "$W/libR10c.so" -DIFUNC $R -lU10
lib T11 "$W/libT11.so" -DTLS11
$CC -std=gnu17 -O1 -w -rdynamic -pthread -o "$W/probe" "$SRC/probe.c" || exit 2
$CC -std=gnu17 -O1 -w -fno-PIE -no-pie -rdynamic -pthread -o "$W/probe_nopie" "$SRC/probe.c" || exit 2

# ---- record ----
san() { sed "s#$W#<dir>#g"; }
echo "# DL-16P record (tools/probe/dl/dlopen/run.sh): $(ldd --version 2>&1 | head -n 1); $(uname -m); gcc $($CC -dumpfullversion)"
for p in p1 p2 p3 p4 p5 p6 p7 p8 p9 p10 p11 p12 p13; do
    echo
    timeout -k 5 300 "$W/probe" $p "$W" > "$W/o" 2>&1
    rc=$?
    san < "$W/o"
    [ $rc -eq 0 ] || echo "   probe $p rc=$rc"
done
echo
echo "== P-13 again, the probe built as a non-PIE executable (-no-pie)"
timeout -k 5 300 "$W/probe_nopie" p13 "$W" > "$W/o" 2>&1
rc=$?
sed 1d "$W/o" | san
[ $rc -eq 0 ] || echo "   probe p13 (non-PIE) rc=$rc"
