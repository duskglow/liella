#!/bin/sh
# C conformance suite (implementation-plan.md §4.3).
#
# Each conformance/c/*.c file is a self-checking C program that includes only standard headers
# (the installed glibc 2.39 headers: Liella is ABI-compatible with them) and Liella's own
# `include/` (the native C ABI's `liella/native.h`, D-X.10), links statically
# against Liella's libc.a with Liella's start object, and is started by Liella's own runtime.
# These tests exercise the library exactly as compiled C code does: real headers, real calling
# convention, real data-symbol access, real process start and exit.
#
# Directives, one per comment line in the source (all optional):
#   /* RUN-ARGS: a b c */          arguments after the program name
#   /* RUN-ENV: A=1 B=2 */         the whole environment, exactly (repeated names are kept;
#                                  passed by tools/execenv.c through execve)
#   /* EXPECT-EXIT: n */           the exit status (default 0)
#   /* EXPECT-SIGNAL: n */         the process must die of signal n instead
#   /* EXPECT-STDERR: text */      standard error must contain text (repeatable)
#   /* RUN-SECURE: yes */          run in secure-execution mode (AT_SECURE): a setgid copy in
#                                  $TMPDIR owned by a supplementary group of the caller; skipped
#                                  (and reported) if the caller has no such group
#   /* RUN-WRAP: cmd args */       run under cmd (e.g. `unshare -U --map-auto --map-root-user`,
#                                  a user namespace with several IDs); skipped (and reported)
#                                  if `cmd args true` fails here
#   /* LINK-LIBM: yes */           also link Liella's libm.a (before libc.a), for a program that
#                                  calls a libm export (fesetround, ...); FAIL if it is missing
#   /* LINK-UNWIND: yes */         also link the compiler's unwinder (libgcc_eh)
#   /* LINK-CXX: yes */            the source is C++17 (compiled by $CXX, default c++) and links
#                                  the compiler's static libstdc++ before Liella's libraries;
#                                  use with LINK-LIBM and LINK-UNWIND
#   /* LINK-NOW: yes */            link with -z now -z relro (the GOT, .got.plt included, lies
#                                  in PT_GNU_RELRO)
#   /* LINK-STATIC-PIE: relr */    link a static PIE (-static-pie), relative relocations packed
#                                  (relr) or as RELA entries (rela); it relocates itself
#   /* LINK-HELPERS: a b */        dynamic-link mode: the program links dynamically and runs twice,
#                                  on the reference C library and under Liella's ld.so with
#                                  Liella's libc.so.6; both runs must meet the EXPECT-* directives.
#                                  Each name is a helper source conformance/c/helpers/<name>.c,
#                                  built as a shared object and linked into the program by path
#                                  (DT_NEEDED, in this order, even if unused by name).
#   /* DLOPEN-HELPERS: c d */      dynamic-link mode: helpers built like LINK-HELPERS but not
#                                  linked (the program dlopens them)
#   /* HELPER-FLAGS: c -DX=1 */    extra compiler and linker flags for helper c (repeatable, one
#                                  line per helper), given in conformance/c/helpers (relative
#                                  files resolve there) with -L at the helper directory
#   /* LINK-EXPORT-DYNAMIC: yes */ dynamic-link mode: the program exports its symbols (-rdynamic)
#   /* PENDING-LIELLA: STORY */    dynamic-link mode: Liella's run is expected to fail until STORY
#                                  lands. The reference must pass; Liella's run failing is reported
#                                  PENDING and counted, and Liella's run passing is a FAIL (drop
#                                  the directive). A build failure is a FAIL either way.
#                                  The program finds its helpers in $CONF_HELPERS (also on
#                                  LD_LIBRARY_PATH). See conformance/README.md.
#
# `sh conformance/run.sh [name ...]` runs only the named tests. Until `cargo xtask so` has
# produced libc.a and the start object, the suite reports SKIPPED and succeeds.
set -eu
here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/.." && pwd)
target=${CARGO_TARGET_DIR:-$root/target}
libc="$target/liella/libc.a"
crt="$target/liella/liella-crt1.o"
# A static PIE's start object (D-27.10: its _start relocates the executable first).
rcrt="$target/liella/liella-rcrt1.o"
cc=${CC:-cc}

if [ ! -f "$libc" ] || [ ! -f "$crt" ] || [ ! -f "$rcrt" ]; then
    count=$(ls "$here"/c/*.c 2>/dev/null | wc -l)
    echo "conformance: SKIPPED ($count C tests; run cargo xtask so to build libc.a and the start object)"
    exit 0
fi

directive() { sed -n "s|^/\* $1: \(.*\) \*/\$|\1|p" "$2"; }

# Dynamic-link mode, Liella's side. Liella's ld.so does not search for libraries yet, so every
# object Liella's build of a program needs is named by path: the program and its helpers link
# against a copy of Liella's libc.so.6 without DT_SONAME (through a libc.so script, as an
# installed C library is linked: the library, the host's link-time archive libc_nonshared.a, and
# the loader as needed), and the helpers by path. No other C library is named, so the one that
# runs is Liella's.
ldso="$target/liella/ld-linux-x86-64.so.2"
dynlibc=""
# bypath IN OUT: a copy of the shared object IN whose DT_SONAME entry is retagged DT_CHECKSUM
# (0x6ffffdf8, which loaders ignore), so an object linked against OUT names it by OUT's path.
bypath() {
    off=$(readelf -Wd "$1" | sed -n 's/^Dynamic section at offset \(0x[0-9a-f]*\) .*/\1/p')
    idx=$(readelf -Wd "$1" | awk '$1 ~ /^0x/ { if ($2 == "(SONAME)") { print n + 0; exit } n++ }')
    [ -n "$off" ] && [ -n "$idx" ] && cp "$1" "$2" &&
        printf '\370\375\377\157\000\000\000\000' | dd of="$2" bs=1 seek=$((off + 16 * idx)) conv=notrunc 2>/dev/null &&
        ! readelf -Wd "$2" | grep -qF '(SONAME)'
}
# The libc.so script of Liella's builds, made once per run of this script ("" when the products
# or the host's libc_nonshared.a are missing).
dyn_setup() {
    [ -z "$dynlibc" ] || return 0
    [ -f "$target/liella/libc.so.6" ] && [ -f "$ldso" ] || return 1
    nonshared=$("$cc" -print-file-name=libc_nonshared.a)
    case $nonshared in /*) ;; *) return 1 ;; esac
    d="$here/build/dynlibc.$$"
    mkdir -p "$d" && bypath "$target/liella/libc.so.6" "$d/libc-by-path.so" || return 1
    printf 'GROUP ( "%s" "%s" AS_NEEDED ( "%s" ) )\n' "$d/libc-by-path.so" "$nonshared" "$ldso" > "$d/libc.so"
    dynlibc="$d/libc.so"
}
# build_helper NAME DIR [ARGS...]: conformance/c/helpers/NAME.c as DIR/libNAME.so, with the
# program's HELPER-FLAGS for NAME, then ARGS (the C library of Liella's builds).
build_helper() {
    bh=$1; bd=$2; shift 2
    bflags=$(sed -n "s|^/\* HELPER-FLAGS: $bh \(.*\) \*/\$|\1|p" "$src")
    # shellcheck disable=SC2086
    (cd "$here/c/helpers" && "$cc" -std=c17 -O1 -shared -fPIC "$bh.c" -L "$bd" $bflags "$@" -o "$bd/lib$bh.so.$$") >>"$log" 2>&1 &&
        mv -f "$bd/lib$bh.so.$$" "$bd/lib$bh.so" || { rm -f "$bd/lib$bh.so.$$"; return 1; }
}
# run_dyn EXE DIR SUFFIX: run a dynamic-link program with its helpers in DIR; its status in rc,
# its output in $log.SUFFIX.stdout and .stderr.
run_dyn() {
    set +e
    if [ -n "$envs" ]; then
        # shellcheck disable=SC2086
        "$execenv" $envs "LD_LIBRARY_PATH=$2" "CONF_HELPERS=$2" -- "$1" $args 2>"$log.$3.stderr" >"$log.$3.stdout" 9>&-
    else
        # shellcheck disable=SC2086
        env LD_LIBRARY_PATH="$2" CONF_HELPERS="$2" "$1" $args 2>"$log.$3.stderr" >"$log.$3.stdout" 9>&-
    fi
    rc=$?
    set -e
}
# verdict LABEL STDERR: whether status $rc and STDERR meet the EXPECT-* directives (1 or 0 in
# ok), each miss printed.
verdict() {
    ok=1
    if [ -n "$want_sig" ]; then
        [ "$rc" -eq $((128 + want_sig)) ] || { ok=0; echo "conformance: $1: status $rc, expected signal $want_sig"; }
    else
        [ "$rc" -eq "$want_exit" ] || { ok=0; echo "conformance: $1: status $rc, expected $want_exit"; }
    fi
    directive EXPECT-STDERR "$src" > "$log.want"
    while IFS= read -r text; do
        grep -qF -- "$text" "$2" || { ok=0; echo "conformance: $1: stderr lacks: $text"; }
    done < "$log.want"
}

mkdir -p "$here/build"
execenv="$here/build/execenv"
# Executables are linked under a temporary name and renamed into place: a concurrent run
# (another test binary) may be executing the previous copy, which a rewrite in place would
# make fail with ETXTBSY.
[ "$execenv" -nt "$here/tools/execenv.c" ] ||
    { "$cc" -O1 -o "$execenv.$$" "$here/tools/execenv.c" && mv -f "$execenv.$$" "$execenv"; }
# The link drops the library sections a program never reaches (libc.a has a section per function
# and data object): ~5x smaller programs, far less written per run (CONF-DIET).
cflags="-std=c17 -O1 -static -nostdlib -fno-builtin -fstack-protector-strong -Wl,--gc-sections"
# A program is relinked only when an input changed. Its key: the products' contents, the headers,
# the source, the compiler and the flags. The products' hash is cached by their size, mtime and
# inode, so it is computed once per build of the products, not once per run.
pstat=$(stat -c '%s %y %i' "$libc" "$crt" "$rcrt" | tr '\n' ' ')
pcache="$here/build/.products.key"
if [ -f "$pcache" ] && [ "$(sed -n 1p "$pcache")" = "$pstat" ]; then
    phash=$(sed -n 2p "$pcache")
else
    phash=$(cat "$libc" "$crt" "$rcrt" | sha256sum | cut -d' ' -f1)
    printf '%s\n%s\n' "$pstat" "$phash" > "$pcache.$$" && mv -f "$pcache.$$" "$pcache"
fi
ihash=$(cd "$root" && find include -type f | LC_ALL=C sort | xargs cat | sha256sum | cut -d' ' -f1)
pass=0; fail=0; pending=0
# A set-group-ID copy (RUN-SECURE) and the libc.so script of dynamic-link mode are removed even
# if the runner stops in the middle of a test.
trap '[ -z "${secure:-}" ] || rm -f "$secure"; rm -rf "$here/build/dynlibc.$$"' EXIT
# Test names (file names without .c) never contain blanks; the repository path may.
if [ $# -eq 0 ]; then
    set -- $(cd "$here/c" && ls *.c | sed 's/\.c$//')
fi
for name in "$@"; do
    src="$here/c/$name.c"
    exe="$here/build/$name"
    log="$here/build/$name.log"
    # One run of a program at a time across processes (nextest runs tests that share a program
    # concurrently): its executable, logs and RUN-SECURE copy are per name. Opening fd 9 again
    # for the next name releases this one; the program itself does not inherit it.
    exec 9>"$exe.lock"
    if command -v flock >/dev/null 2>&1; then flock 9; fi
    # LINK-LIBM: Liella's libm.a goes before libc.a (its undefined symbols resolve in libc.a);
    # its hash joins the key, so a rebuilt libm.a relinks the program.
    libm=""; mhash=""
    if [ -n "$(directive LINK-LIBM "$src")" ]; then
        libm="$target/liella/libm.a"
        if [ ! -f "$libm" ]; then
            echo "conformance: FAIL $name (LINK-LIBM: $libm is missing; run cargo xtask so)"
            fail=$((fail + 1))
            continue
        fi
        mhash=$(sha256sum < "$libm" | cut -d' ' -f1)
    fi
    # LINK-UNWIND: the compiler's unwinder (libgcc_eh) for a program that raises exceptions through
    # the library, with the index it finds call-frame information by (a static link has none
    # otherwise); libc.a again after it, for the unwinder's own references.
    unwind=""
    if [ -n "$(directive LINK-UNWIND "$src")" ]; then
        unwind="-Wl,--eh-frame-hdr -lgcc_eh $libc"
    fi
    # LINK-CXX: a C++ program (std::call_once, exceptions): the C++ compiler and its static
    # runtime; "-x none" makes the libraries after the source plain link inputs again.
    comp=$cc; flags=$cflags; lang=""; lang_end=""; cxxlibs=""; cxxbegin=""; cxxend=""
    if [ -n "$(directive LINK-CXX "$src")" ]; then
        comp=${CXX:-c++}; flags=$(printf '%s' "$cflags" | sed 's/-std=c17/-std=c++17/')
        lang="-x c++"; lang_end="-x none"; cxxlibs="-lstdc++"
        # The compiler's static-program begin/end objects (__dso_handle, which libstdc++ uses).
        cxxbegin=$("$comp" -print-file-name=crtbeginT.o); cxxend=$("$comp" -print-file-name=crtend.o)
    fi
    # LINK-NOW: bind now, so the linker puts the whole GOT (with the IRELATIVE target words of the
    # program's own ifuncs) inside PT_GNU_RELRO; without it they sit in .got.plt, outside RELRO.
    if [ -n "$(directive LINK-NOW "$src")" ]; then
        flags="$flags -Wl,-z,now -Wl,-z,relro"
    fi
    # LINK-STATIC-PIE: a static PIE that relocates itself at start (D-27.10), its relative
    # relocations as RELA entries (rela) or packed (relr, -z pack-relative-relocs). It starts from
    # liella-rcrt1.o, as a -static-pie link with the system's C library starts from its rcrt1.o.
    start=$crt
    case "$(directive LINK-STATIC-PIE "$src")" in
        "") ;;
        rela) flags="$(printf '%s' "$flags" | sed 's/ -static / -static-pie -fPIE /')"; start=$rcrt ;;
        relr) flags="$(printf '%s' "$flags" | sed 's/ -static / -static-pie -fPIE /') -Wl,-z,pack-relative-relocs"; start=$rcrt ;;
        *) echo "conformance: FAIL $name (LINK-STATIC-PIE: rela or relr)"; fail=$((fail + 1)); continue ;;
    esac
    # A C++ static PIE takes the compiler's position-independent begin/end objects, as the
    # compiler's own -static-pie link does (crtbeginT.o is for a fixed-address static program).
    if [ "$start" = "$rcrt" ] && [ -n "$cxxbegin" ]; then
        cxxbegin=$("$comp" -print-file-name=crtbeginS.o); cxxend=$("$comp" -print-file-name=crtendS.o)
    fi
    # LINK-HELPERS / DLOPEN-HELPERS: dynamic-link mode. Two builds of the helpers and the program:
    # the reference's (the host's C library and loader) and Liella's (Liella's libc.so.6 by path,
    # Liella's ld.so as the program interpreter). Each helper is checked to exist after its build,
    # in both sets: a helper left unbuilt fails the test, whichever run would have noticed.
    dyn=""; linked=$(directive LINK-HELPERS "$src"); opened=$(directive DLOPEN-HELPERS "$src")
    helpers="$linked $opened"
    if [ -n "$linked" ] || [ -n "$opened" ]; then
        dyn=1; : > "$log"
        hdir="$exe.helpers"; ldir="$exe.helpers-liella"; mkdir -p "$hdir" "$ldir"
        rm -f "$hdir"/*.so "$ldir"/*.so
        hbad=""
        dyn_setup || hbad="no libc.so.6, ld.so or host libc_nonshared.a for Liella's build; run cargo xtask so"
        for h in $helpers; do
            [ -z "$hbad" ] || break
            if [ ! -f "$here/c/helpers/$h.c" ]; then hbad="missing helper source conformance/c/helpers/$h.c"; break; fi
            build_helper "$h" "$hdir" || { hbad="helper $h does not build"; break; }
            build_helper "$h" "$ldir" -nodefaultlibs "$dynlibc" -lgcc || { hbad="helper $h does not build against Liella"; break; }
            [ -f "$hdir/lib$h.so" ] && [ -f "$ldir/lib$h.so" ] || { hbad="helper $h was not built"; break; }
        done
        rdyn=""; [ -z "$(directive LINK-EXPORT-DYNAMIC "$src")" ] || rdyn="-rdynamic"
        pend=$(directive PENDING-LIELLA "$src")
        # A pending program may call what Liella does not export yet: its run fails, not its link.
        unres=""; [ -z "$pend" ] || unres="-Wl,--unresolved-symbols=ignore-all"
        # The linked helpers by path, one per line (the repository path may hold blanks):
        # --no-as-needed keeps a helper the program reaches only through dlsym as DT_NEEDED.
        hlibs=""; llibs=""
        for h in $linked; do
            hlibs="$hlibs
$hdir/lib$h.so"; llibs="$llibs
$ldir/lib$h.so"
        done
        if [ -z "$hbad" ]; then
            oldifs=$IFS; IFS='
'
            # shellcheck disable=SC2086
            if ! "$cc" -std=c17 -O1 $rdyn -I "$root/include" "$src" -Wl,--no-as-needed $hlibs -o "$exe.$$" >>"$log" 2>&1 \
                  || ! mv -f "$exe.$$" "$exe"; then
                rm -f "$exe.$$"; hbad="does not build"
            # shellcheck disable=SC2086
            elif ! "$cc" -std=c17 -O1 $rdyn -nodefaultlibs "-Wl,--dynamic-linker=$ldso" $unres -I "$root/include" "$src" \
                  -Wl,--no-as-needed $llibs "$dynlibc" -lgcc -o "$exe-liella.$$" >>"$log" 2>&1 \
                  || ! mv -f "$exe-liella.$$" "$exe-liella"; then
                rm -f "$exe-liella.$$"; hbad="does not build against Liella"
            fi
            IFS=$oldifs
        fi
        if [ -n "$hbad" ]; then
            echo "conformance: FAIL $name ($hbad; see conformance/build/$name.log)"
            fail=$((fail + 1))
            continue
        fi
    fi
    key="$phash $mhash $ihash $(sha256sum < "$src" | cut -d' ' -f1) $comp $flags $lang $cxxlibs $unwind ${cxxbegin:-} ${cxxend:-}"
    if [ -n "$dyn" ] || { [ -x "$exe" ] && [ -f "$exe.key" ] && [ "$(cat "$exe.key")" = "$key" ]; }; then
        :   # unchanged: no relink, nothing written
    # shellcheck disable=SC2086
    elif ! "$comp" $flags -I "$root/include" "$start" $cxxbegin $lang "$src" $lang_end $cxxlibs $libm "$libc" $unwind -lgcc $cxxend -o "$exe.$$" >"$log" 2>&1 \
          || ! mv -f "$exe.$$" "$exe" || ! printf '%s\n' "$key" > "$exe.key"; then
        rm -f "$exe.$$" "$exe.key"
        echo "conformance: FAIL $name (does not build; see conformance/build/$name.log)"
        fail=$((fail + 1))
        continue
    fi
    args=$(directive RUN-ARGS "$src")
    envs=$(directive RUN-ENV "$src")
    wrap=$(directive RUN-WRAP "$src")
    if [ -n "$wrap" ]; then
        # shellcheck disable=SC2086
        if ! $wrap true >/dev/null 2>&1; then
            echo "conformance: SKIP $name (RUN-WRAP unavailable: $wrap)"
            continue
        fi
    fi
    want_exit=$(directive EXPECT-EXIT "$src"); want_exit=${want_exit:-0}
    want_sig=$(directive EXPECT-SIGNAL "$src")
    # Dynamic-link mode: the reference's run, then Liella's, each judged as a static program is.
    if [ -n "$dyn" ]; then
        run_dyn "$exe" "$hdir" ref
        verdict "$name (reference)" "$log.ref.stderr"
        if [ "$ok" -ne 1 ]; then
            echo "conformance: FAIL $name on the reference (stderr: conformance/build/$name.log.ref.stderr)"
            fail=$((fail + 1))
            continue
        fi
        run_dyn "$exe-liella" "$ldir" liella
        verdict "$name (Liella)" "$log.liella.stderr"
        if [ -n "$pend" ] && [ "$ok" -eq 1 ]; then
            echo "conformance: FAIL $name (PENDING-LIELLA: $pend, but Liella's run meets the expectations now: remove the directive and the pending mark of its clause test)"
            fail=$((fail + 1))
        elif [ -n "$pend" ]; then
            echo "conformance: PENDING $name on Liella until $pend (fails as expected; stderr: $(head -c 200 "$log.liella.stderr" | head -n 1))"
            pass=$((pass + 1)); pending=$((pending + 1))
        elif [ "$ok" -eq 1 ]; then
            pass=$((pass + 1))
        else
            echo "conformance: FAIL $name on Liella (stderr: conformance/build/$name.log.liella.stderr)"
            fail=$((fail + 1))
        fi
        continue
    fi
    if [ -n "$(directive RUN-SECURE "$src")" ]; then
        group=$(id -Gn | tr ' ' '\n' | grep -vx "$(id -gn)" | head -n 1)
        if [ -z "$group" ]; then
            echo "conformance: SKIP $name (secure mode needs a supplementary group)"
            continue
        fi
        secure="${TMPDIR:-/tmp}/liella-conformance-$name-$$"   # per run: concurrent gates shared one path and removed each other's copy
        cp "$exe" "$secure" && chgrp "$group" "$secure" && chmod g+s "$secure" && exe="$secure"
    fi
    set +e
    if [ -n "$envs" ]; then
        # shellcheck disable=SC2086
        "$execenv" $envs -- "$exe" $args 2>"$log.stderr" >"$log.stdout" 9>&-
    else
        # shellcheck disable=SC2086
        $wrap "$exe" $args 2>"$log.stderr" >"$log.stdout" 9>&-
    fi
    rc=$?
    set -e
    # The set-group-ID copy is not left behind in $TMPDIR.
    if [ "$exe" = "${secure:-}" ]; then
        rm -f "$secure"
    fi
    verdict "$name" "$log.stderr"
    if [ "$ok" -eq 1 ]; then
        pass=$((pass + 1))
    else
        echo "conformance: FAIL $name (stderr: conformance/build/$name.log.stderr)"
        fail=$((fail + 1))
    fi
done
# The pending count (PENDING-LIELLA programs, counted in passed) is reported apart.
if [ "$pending" -gt 0 ]; then
    echo "conformance: $pass passed, $fail failed ($pending of them pending on Liella)"
else
    echo "conformance: $pass passed, $fail failed"
fi
[ "$fail" -eq 0 ]
