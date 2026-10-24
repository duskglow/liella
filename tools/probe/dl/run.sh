#!/bin/sh
# run.sh - DL-0b black-box probes of the reference dynamic loader. Builds the corpus from
# dllib.c and dlprobe.c in SCRATCH (created, must not exist), runs every probe, prints the
# record on stdout with SCRATCH shown as <dir>. Public interfaces and documented loader
# options only; the reference's own files are only executed, and its libraries' exported
# names listed (nm -D, the public ABI).
#   sh tools/probe/dl/run.sh SCRATCH > spec/behaviour/dynlink.glibc.txt
set -u
SRC=$(cd "$(dirname "$0")" && pwd)
W=${1:?usage: run.sh SCRATCH}
[ -e "$W" ] && { echo "run.sh: $W exists" >&2; exit 2; }
mkdir -p "$W/subdir" "$W/tls" "$W/gone" "$W/gone2" || exit 2
W=$(cd "$W" && pwd)
CC=${CC:-cc}
CF="-std=gnu17 -O1 -fPIC -w"
LDSO=$(readlink -f /lib64/ld-linux-x86-64.so.2)
export LC_ALL=C
unset LD_LIBRARY_PATH LD_PRELOAD LD_BIND_NOW GLIBC_TUNABLES 2>/dev/null

lib() { # lib NAME OUT [extra cc args...]
    lib_n=$1; lib_o=$2; shift 2
    $CC $CF -shared -DNAME="$lib_n" -o "$lib_o" "$SRC/dllib.c" -L"$W" -Wl,-rpath,"$W" -Wl,--no-as-needed "$@" || { echo "build failed: $lib_o" >&2; exit 2; }
}
patch() { # patch FILE OFFSET OCTAL-BYTE
    printf "\\$3" | dd of="$1" bs=1 seek="$2" conv=notrunc 2>/dev/null
}

# ---- corpus ----
lib C "$W/libC.so"
lib B "$W/libB.so" -DCALL=C -lC
lib A "$W/libA.so" -DCALL=B -DWITH_PRIO -lB -lC
lib D "$W/libD.so" -DCALL=C -DWITH_INIT -Wl,-init=lib_init,-fini=lib_fini -lC
lib F "$W/libF.so"
lib E "$W/libE.so" -DCALL=F -DWITH_ATEXIT -lC -lF
lib I "$W/libI.so"
lib H "$W/libH.so" -DCALL=I -lI
lib I "$W/libI.so" -DCALL=H -lH
$CC -std=gnu17 -O1 -w -DORDER_MAIN -o "$W/dlorder" "$SRC/dlprobe.c" -L"$W" -Wl,--no-as-needed -lA -lD -ldl -Wl,-rpath,"$W" || exit 2
$CC -std=gnu17 -O1 -w -o "$W/dlprobe" "$SRC/dlprobe.c" -ldl || exit 2

echo "not an object" > "$W/text.so"
: > "$W/empty.so"
printf '\177ELF\002\001\001' > "$W/short.so"
for f in class32 machine msb osabi; do cp "$W/libC.so" "$W/$f.so"; done
patch "$W/class32.so" 4 001
patch "$W/msb.so" 5 002
patch "$W/osabi.so" 7 141
patch "$W/machine.so" 18 050
head -c 200 "$W/libC.so" > "$W/trunc.so"
$CC $CF -DNAME=C -c -o "$W/etype_rel.o" "$SRC/dllib.c"
printf 'int main(void){return 0;}\n' > "$W/exe.c"
$CC -O1 -fPIE -pie -o "$W/pie_exe" "$W/exe.c"
$CC -O1 -fno-PIE -no-pie -o "$W/nopie_exe" "$W/exe.c"
lib Gone "$W/gone/libGone.so"
lib MissingDep "$W/libMissingDep.so" -L"$W/gone" -lGone
lib U "$W/libU.so" -DUNDEF
printf 'VER_2 { global: fn_ver; local: *; };\n' > "$W/v2.map"
printf 'VER_1 { global: fn_ver; local: *; };\n' > "$W/v1.map"
lib Ver "$W/libVer.so" -DWITH_VER -Wl,-soname,libVer.so -Wl,--version-script,"$W/v2.map"
lib W "$W/libW.so" -DCALL=ver -lVer
lib R "$W/libR.so" -Wl,--enable-new-dtags,-rpath,'/r/$LIB/$PLATFORM/$ORIGIN/${LIB}'
n=64
while [ $n -le 8192 ]; do lib T "$W/tls/libtls$n.so" -DTLS_IE_SIZE=$n; n=$((n + 64)); done
i=0
while [ $i -lt 64 ]; do lib "tlsc$i" "$W/tls/libtlsc$i.so" -DTLS_IE_SIZE=128; i=$((i + 1)); done

# programs for the loader's own exit status and messages
printf '#include <stdio.h>\nint fn_Gone2(void);\nint main(void){puts("main");fflush(stdout);return fn_Gone2()==1?0:1;}\n' > "$W/pg.c"
lib Gone2 "$W/gone2/libGone2.so"
$CC -O1 -o "$W/prog_missing" "$W/pg.c" -L"$W/gone2" -lGone2 || exit 2
printf '#include <stdio.h>\nint fn_U2(void);\nint main(void){puts("main before call");fflush(stdout);fn_U2();puts("after");return 0;}\n' > "$W/pu.c"
lib U2 "$W/libU2.so" -Wl,-soname,libU2.so
$CC -O1 -o "$W/prog_lazy" "$W/pu.c" -L"$W" -lU2 -Wl,-z,lazy -Wl,-rpath,"$W" || exit 2
$CC -O1 -o "$W/prog_now" "$W/pu.c" -L"$W" -lU2 -Wl,-z,now -Wl,-rpath,"$W" || exit 2
lib U2x "$W/libU2.so" -Wl,-soname,libU2.so
printf '#include <stdio.h>\nint fn_ver(void);\nint main(void){puts("main");return fn_ver()==2?0:1;}\n' > "$W/pv.c"
$CC -O1 -o "$W/prog_ver" "$W/pv.c" -L"$W" -lVer -Wl,-rpath,"$W" || exit 2

# the dependency and the newer version disappear after linking
rm -f "$W/gone/libGone.so" "$W/gone2/libGone2.so"
lib Ver "$W/libVer.so" -DWITH_VER -Wl,-soname,libVer.so -Wl,--version-script,"$W/v1.map"

# ---- record ----
san() { sed "s#$W#<dir>#g"; }
echo "# DL-0b record: $(ldd --version 2>&1 | head -n 1); $(uname -m); loader $LDSO"
echo
"$W/dlprobe" errors "$W" | san
echo
"$W/dlprobe" phdr "$W" | san
echo
"$W/dlprobe" tls "$W" | san
echo
"$W/dlprobe" subst "$W" | san
echo
"$W/dlprobe" compat | san
echo
for m in close exit _exit cycle; do
    echo "== order: dlorder $m (main linked -lA -lD; A needs B, C; D needs C; E needs C, F)"
    "$W/dlorder" $m "$W" > "$W/o" 2>&1
    rc=$?
    san < "$W/o"
    echo "   rc=$rc"
done
echo
run() { # run LABEL CMD... : rc, stdout, stderr (each line prefixed)
    l=$1; shift
    "$@" > "$W/o" 2> "$W/e"
    rc=$?
    echo "-- $l | rc=$rc"
    head -n 6 "$W/o" | san | sed 's/^/   out: /'
    head -n 6 "$W/e" | san | sed 's/^/   err: /'
}
echo "== loader: exit status and messages"
run "missing DT_NEEDED" "$W/prog_missing"
$CC -O1 -static -o "$W/static_exe" "$W/exe.c" 2>/dev/null && run "ld.so --verify static_exe" "$LDSO" --verify "$W/static_exe"
run "ld.so --verify libC.so" "$LDSO" --verify "$W/libC.so"
run "lazy undefined function, -z lazy" "$W/prog_lazy"
run "lazy undefined function, -z now" "$W/prog_now"
run "lazy undefined, LD_BIND_NOW=1" env LD_BIND_NOW=1 "$W/prog_lazy"
run "version VER_2 not found" "$W/prog_ver"
run "ld.so with no arguments" "$LDSO"
run "ld.so nonexistent program" "$LDSO" "$W/nope"
run "ld.so text file" "$LDSO" "$W/text.so"
run "ld.so directory" "$LDSO" "$W/subdir"
run "ld.so class32 object" "$LDSO" "$W/class32.so"
run "ld.so a shared library (no main)" "$LDSO" "$W/libC.so"
run "ld.so pie_exe" "$LDSO" "$W/pie_exe"
run "ld.so nopie_exe" "$LDSO" "$W/nopie_exe"
run "ld.so --verify dlorder" "$LDSO" --verify "$W/dlorder"
run "ld.so --verify text.so" "$LDSO" --verify "$W/text.so"
run "ld.so --list prog_missing" "$LDSO" --list "$W/prog_missing"
run "ld.so --bogus-option" "$LDSO" --bogus-option "$W/pie_exe"
run "ld.so --version" "$LDSO" --version
run "ld.so --help" "$LDSO" --help
echo
echo "== compat: exported names of the placeholder libraries (nm -D --defined-only, name@version)"
for l in libdl.so.2 libpthread.so.0 librt.so.1 libutil.so.1 libanl.so.1; do
    p=$(readlink -f "/lib/x86_64-linux-gnu/$l")
    echo "-- $l: $(nm -D --defined-only --with-symbol-versions "$p" 2>/dev/null | awk '{print $3}' | sort | tr '\n' ' ')"
done
echo
echo "== bindnow: regular ELF files in /usr/bin (symlinks not followed)"
tot=0; st=0; now=0; lazy=0; pie=0
for f in /usr/bin/*; do
    [ -f "$f" ] && [ ! -L "$f" ] || continue
    h=$(readelf -h "$f" 2>/dev/null) || continue
    case $h in *"ELF64"*) ;; *) continue ;; esac
    tot=$((tot + 1))
    case $h in *"DYN ("*) pie=$((pie + 1)) ;; esac
    d=$(readelf -d "$f" 2>/dev/null)
    case $d in
        *"There is no dynamic section"*|"") st=$((st + 1)) ;;
        *BIND_NOW*|*"Flags: NOW"*|*"Flags: NOW "*|*" NOW "*) now=$((now + 1)) ;;
        *) lazy=$((lazy + 1)) ;;
    esac
done
echo "elf64=$tot static=$st dynamic_bind_now=$now dynamic_lazy=$lazy type_dyn(pie)=$pie"
