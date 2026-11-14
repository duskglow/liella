#!/bin/sh
# validation/smoke/check-elf.sh LDSO SYSROOT PATH...: check that every ELF program and shared object under the
# PATHs (files or directories) was linked against Liella by liella-cc, so a test run cannot pass on the
# system's C library by accident.
#
#   program (EXEC, or DYN with PT_INTERP)  PT_INTERP is LDSO and DT_NEEDED names SYSROOT/libc-by-path.so
#   shared object (DYN, no PT_INTERP)      DT_NEEDED names no C library other than Liella's by-path copies
#   static program (EXEC, no PT_INTERP)    rejected: it carries some other C library
#   any object                             DT_NEEDED never names libc.so.6, libm.so.6, libpthread.so.0, ld-linux*
#                                          by soname (the system's libraries would be the ones a search finds)
# A DT_NEEDED soname of another library (libz.so.1, ...) is not a C-library fallback; it is listed as
# "needs-search" (the loader's start-time library search, not available yet) and does not fail the check.
#
# Prints one line per failing object ("smoke-check: FAIL <file>: <why>"), one per needs-search object, and
# a summary line; exits 1 when any object failed, 0 otherwise.
set -u
[ $# -ge 3 ] || { echo "usage: check-elf.sh LDSO SYSROOT PATH..." >&2; exit 2; }
ldso=$1; sys=$2; shift 2
libc="$sys/libc-by-path.so"
fail=0; ok=0; search=0
list=$(mktemp) || exit 2
trap 'rm -f "$list"' EXIT
for p in "$@"; do
    find "$p" -type f \( -perm -u+x -o -name '*.so' -o -name '*.so.*' \) ! -name '*.o' ! -name '*.a' 2>/dev/null
done | sort > "$list"
while IFS= read -r f; do
    hdr=$(readelf -h "$f" 2>/dev/null) || continue
    type=$(printf '%s\n' "$hdr" | sed -n 's/^ *Type: *\([A-Z]*\).*/\1/p')
    case $type in EXEC | DYN) ;; *) continue ;; esac
    interp=$(readelf -Wl "$f" 2>/dev/null | sed -n 's/.*\[Requesting program interpreter: \(.*\)\]$/\1/p')
    needed=$(readelf -Wd "$f" 2>/dev/null | sed -n 's/.*(NEEDED) *Shared library: \[\(.*\)\]$/\1/p')
    why=""
    for n in $needed; do
        case $n in
            libc.so.6 | libm.so.6 | libpthread.so.0 | libdl.so.2 | librt.so.1 | ld-linux*) why="needs $n by soname (the system's library)" ;;
        esac
    done
    if [ -z "$why" ]; then
        if [ -n "$interp" ]; then
            if [ "$interp" != "$ldso" ]; then
                why="PT_INTERP is $interp, not Liella's loader"
            elif ! printf '%s\n' "$needed" | grep -qxF "$libc"; then
                why="DT_NEEDED does not name Liella's libc ($libc)"
            fi
        elif [ "$type" = EXEC ]; then
            why="static program (no PT_INTERP): not Liella's dynamic link"
        fi
    fi
    if [ -n "$why" ]; then
        echo "smoke-check: FAIL $f: $why"
        fail=$((fail + 1))
        continue
    fi
    ok=$((ok + 1))
    other=$(printf '%s\n' "$needed" | grep -v '^/' | grep -v '^$' | tr '\n' ' ')
    if [ -n "$other" ]; then
        echo "smoke-check: needs-search $f: $other"
        search=$((search + 1))
    fi
done < "$list"
echo "smoke-check: $ok on Liella ($search need the library search), $fail failed"
[ $fail -eq 0 ]
