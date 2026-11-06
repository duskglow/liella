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
#   /* LINK-HELPERS: a b */        dynamic-link mode: the test links dynamically, against the
#                                  reference C library only (no Liella loader yet). Each name
#                                  is a helper source conformance/c/helpers/<name>.c, built as
#                                  build/<test>.helpers/lib<name>.so and linked into the program
#                                  (DT_NEEDED, even if unused by name); the program runs with
#                                  LD_LIBRARY_PATH set to that directory. The result is the
#                                  reference's; Liella is reported SKIPPED with the reason.
#                                  See conformance/README.md.
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
pass=0; fail=0
# A set-group-ID copy (RUN-SECURE) is removed even if the runner stops in the middle of a test.
trap '[ -z "${secure:-}" ] || rm -f "$secure"' EXIT
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
    # LINK-HELPERS: dynamic-link mode, reference C library only (Liella has no loader yet).
    dyn=""; helpers=$(directive LINK-HELPERS "$src")
    if [ -n "$helpers" ]; then
        dyn=1
        hdir="$exe.helpers"; mkdir -p "$hdir"
        hlibs=""; hbad=""
        for h in $helpers; do
            hsrc="$here/c/helpers/$h.c"
            if [ ! -f "$hsrc" ]; then hbad="missing helper source $hsrc"; break; fi
            if ! "$cc" -std=c17 -O1 -shared -fPIC "$hsrc" -o "$hdir/lib$h.so.$$" >"$log" 2>&1 \
                  || ! mv -f "$hdir/lib$h.so.$$" "$hdir/lib$h.so"; then
                rm -f "$hdir/lib$h.so.$$"; hbad="helper $h does not build"; break
            fi
            hlibs="$hlibs -l$h"
        done
        # --no-as-needed: a test that reaches its helper only through dlsym still gets it as DT_NEEDED.
        # shellcheck disable=SC2086
        if [ -z "$hbad" ]; then
            if ! "$cc" -std=c17 -O1 -I "$root/include" "$src" -L "$hdir" -Wl,--no-as-needed $hlibs -o "$exe.$$" >>"$log" 2>&1 \
                  || ! mv -f "$exe.$$" "$exe"; then
                rm -f "$exe.$$"; hbad="does not build"
            fi
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
    if [ -n "$(directive RUN-SECURE "$src")" ]; then
        group=$(id -Gn | tr ' ' '\n' | grep -vx "$(id -gn)" | head -n 1)
        if [ -z "$group" ]; then
            echo "conformance: SKIP $name (secure mode needs a supplementary group)"
            continue
        fi
        secure="${TMPDIR:-/tmp}/liella-conformance-$name-$$"   # per run: concurrent gates shared one path and removed each other's copy
        cp "$exe" "$secure" && chgrp "$group" "$secure" && chmod g+s "$secure" && exe="$secure"
    fi
    if [ -n "$dyn" ]; then
        if [ -n "$envs" ]; then envs="$envs LD_LIBRARY_PATH=$hdir"; else export LD_LIBRARY_PATH="$hdir"; fi
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
    ok=1
    if [ -n "$want_sig" ]; then
        [ "$rc" -eq $((128 + want_sig)) ] || { ok=0; echo "conformance: $name: status $rc, expected signal $want_sig"; }
    else
        [ "$rc" -eq "$want_exit" ] || { ok=0; echo "conformance: $name: status $rc, expected $want_exit"; }
    fi
    directive EXPECT-STDERR "$src" > "$log.want"
    while IFS= read -r text; do
        grep -qF -- "$text" "$log.stderr" || { ok=0; echo "conformance: $name: stderr lacks: $text"; }
    done < "$log.want"
    if [ "$ok" -eq 1 ] && [ -n "$dyn" ]; then
        echo "conformance: SKIPPED $name for Liella (passed on the reference; Liella has no dynamic loader yet)"
        pass=$((pass + 1))
    elif [ "$ok" -eq 1 ]; then
        pass=$((pass + 1))
    else
        echo "conformance: FAIL $name (stderr: conformance/build/$name.log.stderr)"
        fail=$((fail + 1))
    fi
done
echo "conformance: $pass passed, $fail failed"
[ "$fail" -eq 0 ]
