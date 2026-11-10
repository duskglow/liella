#!/bin/bash
# Regenerate the hot-text list (PAGEFAULTS-2, crates/libc-so/hot-text.list) on a Linux host with gdb.
# usage: tools/hottext/regen.sh <tree with built products> [--replace] ['<static program> <args>'...]
#   Traces the startup / stdio / setlocale+printf probes (and any extra static programs linked
#   against <tree>/target/liella/libc.a) with trace_gdb.py, keeps the names that are function
#   sections of libc.a, and merges their keys into the list (--replace: start from an empty list).
#   Run it once per profile (`cargo xtask so` = fast, LIELLA_FULL_PROFILE=1 = release): inlining
#   differs, and the list is the union. Liella binaries only; never trace a glibc-linked program.
#   HOTTEXT-STARTUP-COMPLETE: each line is `<key>\t<tier>\t<profiles>`: the tier of the first probe
#   that touched it (startup p_true, fork p_fork, stdio p_puts/p_cat/p_fmt, strings p_str, locale
#   p_loc, tool p_locuse/p_nss and the extra programs, extra anything else) and the profiles whose
#   trace hit it (fast, release; read from <tree>/target/liella/.profile). xtask/src/hottext.rs
#   fails the build when a startup- or fork-tier key of the profile being built matches no section.
#   HOT-ORDER: the list is a LAYOUT ORDER, not a set: keys in first-touch order (startup first, then
#   the probes in the order below, then the extra programs); xtask/src/hottext.rs numbers the
#   sections so the link lays the group out in that order. merge.py keeps an older list's keys.
#   HOTTEXT-REGEN-UNBLOCK: --build (needs <tree> = this script's tree) builds the products itself, in the
#   profile LIELLA_FULL_PROFILE selects, with `cargo xtask so --hottext-regen`: the dead-key check is
#   reported, not enforced, so a stale key no longer blocks its own repair. After rewriting the list it
#   rebuilds with plain `cargo xtask so`, which must pass the check. Nothing else may pass that flag
#   (`cargo xtask static`). Both profiles: `regen.sh . --build` then `LIELLA_FULL_PROFILE=1 regen.sh . --build`.
set -eu
here=$(cd "$(dirname "$0")" && pwd)
T=$(cd "$1" && pwd); shift
list=$here/../../crates/libc-so/hot-text.list
replace=0
build=0
while [ $# -gt 0 ]; do
    case $1 in --replace) replace=1; shift;; --build) build=1; shift;; *) break;; esac
done
if [ $build = 1 ]; then
    if [ "$T" != "$(cd "$here/../.." && pwd)" ]; then echo "regen: --build builds the list's own tree; give $(cd "$here/../.." && pwd)" >&2; exit 1; fi
    (cd "$T" && cargo xtask so --hottext-regen) || { echo "regen: product build failed" >&2; exit 1; }
fi
profile=$(cat "$T/target/liella/.profile" 2>/dev/null || true)
case $profile in fast|release) ;; *) echo "regen: $T/target/liella/.profile: want fast or release, got '$profile' (build the products first)" >&2; exit 1;; esac
work=$(mktemp -d "${TMPDIR:-/tmp}/hottext.XXXXXX")
trap 'rm -rf "$work"' EXIT
cp "$here"/probes/*.c "$work"/
echo x > "$work/in.txt"
cd "$work"
n=0
tier_of() { # <probe> -> its tier (HOTTEXT-STARTUP-COMPLETE)
    case $1 in
        p_true) echo startup;; p_fork) echo fork;; p_puts|p_cat|p_fmt) echo stdio;; p_str) echo strings;;
        p_loc) echo locale;; p_locuse|p_nss) echo tool;; *) echo extra;;
    esac
}
: > "$work/tagged"
trace() { # <tier> <LANG> <program> <args...>
    local tier=$1 lang=$2; shift 2
    n=$((n + 1))
    printf '%s=%s\n' "$tier" "$work/names.$n" >> "$work/tagged"
    LANG=$lang LC_ALL= HOT_TEXT_OUT=$work/names.$n HOT_TEXT_STDIN=${stdin:-$work/in.txt} HOT_TEXT_FOLLOW_FORK=${follow:-} timeout 900 \
        gdb -nx -batch -x "$here/trace_gdb.py" --args "$@" > "$work/gdb.$n.log" 2>&1 || { tail -5 "$work/gdb.$n.log"; exit 1; }
}
# Startup (p_true) first, then the probes from the most common calls to the rarer ones (printf before
# setlocale: HOT-ORDER-2, a printf-first program spans fewer windows); any new
# probe not named here goes last (alphabetically).
probes=$(for p in p_true p_puts p_cat p_str p_fmt p_loc p_cold; do [ -f $p.c ] && echo $p; done
         for c in *.c; do case " p_true p_puts p_cat p_str p_fmt p_loc p_cold " in *" ${c%.c} "*) ;; *) echo "${c%.c}";; esac; done)
# HOT-ORDER-3 guard: every library function a probe's source names must have run in its trace. gcc
# folds or replaces libc calls with constant arguments (p_cold's strcoll at -O2), and the list then
# silently misses the function; call it through a volatile pointer instead.
nm --defined-only -g "$T/target/liella/libc.a" 2>/dev/null | awk 'NF == 3 && $2 ~ /^[TWi]$/ {print $3}' | sort -u > "$work/libfns"
check_calls() { # <probe>: after its trace (names.$n)
    local missing
    # A fortified call (printf -> __printf_chk) and a header redirect (libintl.h at -O2: dgettext ->
    # __dcgettext) count as the call.
    { cat "$work/names.$n"; sed -En 's/^__(.*)_chk$/\1/p' "$work/names.$n"
      if grep -qx __dcgettext "$work/names.$n"; then printf '%s\n' gettext dgettext dcgettext; fi; } > "$work/ran"
    missing=$(gcc -fpreprocessed -E "$1.c" 2>/dev/null | grep -v '^#' | grep -oE '[A-Za-z_][A-Za-z0-9_]*' | sort -u |
              grep -Fx -f "$work/libfns" | grep -Fxv -f "$work/ran" | tr '\n' ' ' || true)
    if [ -n "$missing" ]; then echo "regen: $1: library functions named in the source never ran: $missing" >&2; exit 1; fi
}
for p in $probes; do
    LIELLA_PRODUCTS=$T/target/liella "$T/scripts/liella-cc" -O2 -o "$p" "$p.c"
    if [ "$p" = p_cat ]; then
        # HOT-ORDER-3: p_cat's read path here (empty stdin), its write path (fwrite) at the end of the
        # group below: fwrite ahead of the printf group cost the first snprintf one fault (Ivy Bridge host B:
        # cold.c snprintf 12 -> 13, cold2 first snprintf 8 -> 9).
        stdin=/dev/null trace stdio en_US.UTF-8 ./p_cat
    elif [ "$p" = p_fork ]; then
        # TIMING-COV-FORK-COW-FAULTS: trace into the fork child (its text windows fault after fork).
        follow=child trace fork en_US.UTF-8 ./p_fork
        check_calls p_fork
    else
        trace "$(tier_of "$p")" en_US.UTF-8 "./$p"
        check_calls "$p"
    fi
done
trace locale C ./p_loc
if [ -f p_cat.c ]; then trace stdio en_US.UTF-8 ./p_cat; check_calls p_cat; fi
for cmd in "$@"; do
    # shellcheck disable=SC2086
    trace tool en_US.UTF-8 $cmd
done
# shellcheck disable=SC2046
python3 "$here/keys.py" --tagged $(cat "$work/tagged") > "$work/hit.keys"
# Only names that are function sections of libc.a (drops the programs' own functions).
# (A grouped archive's sections carry the hot prefix, `.text.sorted.h<n>.` or the older `.text.hot.`;
# a #[cold] function's section is `.text.unlikely.<f>`, HOTTEXT-COLD-STARTUP.)
readelf -SW "$T/target/liella/libc.a" | sed -En 's/^ *\[ *[0-9]*\] \.text\.(sorted\.h[0-9]+\.|hot\.|unlikely\.)?([^ ]*) .*/\2/p' > "$work/sections"
python3 "$here/keys.py" "$work/sections" > "$work/lib.keys"
awk -F'\t' 'NR == FNR { lib[$1]; next } $1 in lib' "$work/lib.keys" "$work/hit.keys" > "$work/new.keys"  # keeps the touch order
if [ $replace = 1 ] || [ ! -f "$list" ]; then : > "$work/old.keys"; else grep -v '^#' "$list" > "$work/old.keys" || true; fi
{
    echo "# Functions grouped as hot text in libc.a (PAGEFAULTS-2), in this order (HOT-ORDER: first touch): build-stable keys, see tools/hottext/keys.py."
    echo "# Generated by tools/hottext/regen.sh; do not edit by hand."
    echo "# <key> TAB <tier> TAB <profiles traced in> (HOTTEXT-STARTUP-COMPLETE; tiers startup fork stdio strings locale tool extra)."
    python3 "$here/merge.py" "$work/old.keys" "$work/new.keys" "$profile"
} > "$list.tmp"
mv "$list.tmp" "$list"
echo "hot-text: $(wc -l < "$work/new.keys") keys traced ($profile), $(grep -vc '^#' "$list") in $list"
if [ $build = 1 ]; then
    # The rewritten list must pass the dead-key check in this profile (merge.py drops the keys this
    # trace no longer hits from this profile).
    (cd "$T" && cargo xtask so) || { echo "regen: the rewritten list still fails cargo xtask so ($profile)" >&2; exit 1; }
fi
