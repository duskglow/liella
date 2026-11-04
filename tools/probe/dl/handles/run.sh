#!/bin/sh
# run.sh - black-box probe of dlsym(RTLD_DEFAULT / RTLD_NEXT) and dlopen(NULL) on the reference
# loader (spec §27.1 s27_1_c_3, s27_1_c_1_3). Builds handles.c against two helpers that both define
# f in SCRATCH (created, must not exist), runs it, then runs two planted variants the checks must
# catch:
#   next-self  helper 1's "next" lookup starts at helper 1 itself  (expect: next f != 2)
#   no-export  the program is linked without -rdynamic, so the global handle has no main_sym
# Prints one line per case; exit 0 only when the real build passes and both plants are CAUGHT.
#   sh tools/probe/dl/handles/run.sh SCRATCH
set -u
SRC=$(cd "$(dirname "$0")" && pwd)
W=${1:?usage: run.sh SCRATCH}
[ -e "$W" ] && { echo "run.sh: $W exists" >&2; exit 2; }
mkdir -p "$W/real" "$W/next-self" "$W/no-export" || exit 2
W=$(cd "$W" && pwd)
CC=${CC:-cc}
export LC_ALL=C
unset LD_LIBRARY_PATH LD_PRELOAD LD_BIND_NOW GLIBC_TUNABLES 2>/dev/null

build() { # build DIR HELPER1-EXTRA MAIN-EXTRA
    d=$W/$1
    $CC -std=gnu17 -O1 -fPIC -shared -DNUM=1 $2 -o "$d/libhandle1.so" "$SRC/handle.c" -ldl &&
    $CC -std=gnu17 -O1 -fPIC -shared -DNUM=2 -o "$d/libhandle2.so" "$SRC/handle.c" &&
    $CC -std=gnu17 -O1 $3 -o "$d/handles" "$SRC/handles.c" -L"$d" -Wl,--no-as-needed -lhandle1 -lhandle2 -ldl -Wl,-rpath,"$d" ||
    { echo "build failed: $1" >&2; exit 2; }
}
run() { # run DIR -> rc; output in DIR/out
    timeout 60 "$W/$1/handles" > "$W/$1/out" 2>&1
}

build real "" -rdynamic
build next-self -DPLANT_NEXT_SELF -rdynamic
build no-export "" ""

ok=0
run real; rc=$?
echo "== real (rc $rc)"; sed "s|$W|<dir>|g" "$W/real/out"
[ "$rc" -eq 0 ] || { echo "real: FAIL"; ok=1; }
for p in next-self no-export; do
    run "$p"; rc=$?
    case $p in next-self) want='n && n() == 2' ;; no-export) want='m == &main_sym' ;; esac
    if [ "$rc" -ne 0 ] && grep -qF "$want" "$W/$p/out"; then
        echo "plant $p: CAUGHT (rc $rc, $want)"
    else
        echo "plant $p: SURVIVED (rc $rc)"; sed "s|$W|<dir>|g" "$W/$p/out"; ok=1
    fi
done
exit $ok
