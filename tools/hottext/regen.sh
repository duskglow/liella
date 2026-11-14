#!/bin/bash
# The trace of a trace-at-build `cargo xtask so` (LAYOUT-TRACE-AT-BUILD, D-X.88 / D-X.92; LAYOUT-DELETE-LISTS, LT-5):
# usage: LIELLA_HOT_LISTS=<lists dir> tools/hottext/regen.sh <tree> --products <dir>
#   `cargo xtask so` runs it on a product finished ungrouped from its own archive (<dir>: libc.a, liella-crt1.o and
#   .profile) and groups the real product from what it writes into <lists dir> (target/product/<profile>):
#   hot-text.list, startup-text.keys, probe-text.keys and startup-data.keys, each written fresh from this
#   build's trace. Nothing of an earlier build is read or merged, and no list is committed (LT-5 deleted them):
#   no function name crosses a build. Run it only through `cargo xtask so`.
#   Traces the startup / stdio / setlocale+printf probes (tools/hottext/probes) and every mode of
#   crates/core/tests/oracle/page_faults.c that crates/core/tests/hot_text_products.rs bounds, with
#   trace_gdb.py, and keeps the names that are function sections of libc.a. Liella binaries only; never trace a
#   glibc-linked program.
#   HOTTEXT-STARTUP-COMPLETE: each line is `<key>\t<tier>\t<profile>`: the tier of the first probe that touched
#   it (startup p_true, fork p_fork, stdio p_puts/p_cat/p_fmt, strings p_str, locale p_loc, tool p_locuse/p_nss,
#   extra anything else).
#   HOT-ORDER: the list is a LAYOUT ORDER, not a set: keys in first-touch order (startup first, then the probes
#   in the order below); xtask/src/hottext.rs numbers the sections so the link lays the group out in that order.
#   HOTLIST-REGEN-CHEAP: the traces go in parallel (HOTTEXT_JOBS at once, default nproc), and each trace keeps
#   its number, so order.py reads them in the order above.
#   LAYOUT-TRACE-AT-BUILD-2: the trace set is checked against the modes of page_faults.c (the modes
#   hot_text_products.rs measures); a mode not traced fails by name.
set -eu
# No __pycache__ in the tree: a compiled module embeds this tree's absolute path, which the
# private-paths check of `cargo xtask static` rejects in a role tree.
export PYTHONDONTWRITEBYTECODE=1
here=$(cd "$(dirname "$0")" && pwd)
T=$(cd "$1" && pwd); shift
L=${LIELLA_HOT_LISTS:-}
if [ -z "$L" ] || [ ! -d "$L" ]; then
    echo "regen: LIELLA_HOT_LISTS must name the lists directory of this build (cargo xtask so sets it; no list is committed, LT-5)" >&2
    exit 1
fi
P=
while [ $# -gt 0 ]; do
    case $1 in --products) P=$(cd "$2" && pwd); shift 2;;
        *) echo "regen: unknown argument $1 (usage: LIELLA_HOT_LISTS=<dir> regen.sh <tree> --products <dir>; run through cargo xtask so)" >&2; exit 1;; esac
done
[ -n "$P" ] || { echo "regen: --products <dir> is required" >&2; exit 1; }
list=$L/hot-text.list

profile=$(cat "$P/.profile" 2>/dev/null || true)
case $profile in fast|release) ;; *) echo "regen: $P/.profile: want fast or release, got '$profile' (build the products first)" >&2; exit 1;; esac
work=$(mktemp -d "${TMPDIR:-/tmp}/hottext.XXXXXX")
trap 'rm -rf "$work"' EXIT
cd "$work"
n=0
# shellcheck source=tools/hottext/runner.sh
. "$here/runner.sh"
# LAYOUT-TRACE-HOST-CANONICAL: every trace reads the tree's data files (canon_host.sh) and runs on a
# virtual CPU per class (trace_gdb.py), so the list is a function of the tree, not of the build host.
canon_prepare
cp "$here"/probes/*.c "$work"/
echo x > "$work/in.txt"
tier_of() { # <probe> -> its tier (HOTTEXT-STARTUP-COMPLETE)
    case $1 in
        p_true) echo startup;; p_fork) echo fork;; p_puts|p_cat|p_fmt) echo stdio;; p_str) echo strings;;
        p_loc) echo locale;; p_locuse|p_nss) echo tool;; *) echo extra;;
    esac
}
: > "$work/tagged"
: > "$work/traced"
trace() { # <tier> <LANG> <program> <args...>; tz=<zone> sets the program's TZ
    # LAYOUT-TRACE-HOST-CANONICAL: LANG (and TZ when given) is the program's whole environment (runner.sh).
    local tier=$1 lang=$2; shift 2
    printf '%s\n' "$*" >> "$work/traced"  # the trace set, for the --all-modes coverage check
    # shellcheck disable=SC2046
    trace_tagged "$tier" LANG="$lang" $([ -n "${tz:-}" ] && echo "TZ=$tz") HOT_TEXT_STDIN="${stdin:-$work/in.txt}" HOT_TEXT_FOLLOW_FORK="${follow:-}" -- "$@"
}
checks=
traces_done() { # wait for every trace, fail on the first that failed, then run the deferred call checks
    traces_wait || exit 1
    for c in $checks; do check_calls "${c%:*}" "${c#*:}"; done
}
# Startup (p_true) first, then the probes from the most common calls to the rarer ones (printf before
# setlocale: HOT-ORDER-2, a printf-first program spans fewer windows); any new
# probe not named here goes last (alphabetically).
probes=$(for p in p_true p_puts p_cat p_str p_fmt p_loc p_cold; do [ -f $p.c ] && echo $p; done
         for c in *.c; do case " p_true p_puts p_cat p_str p_fmt p_loc p_cold " in *" ${c%.c} "*) ;; *) echo "${c%.c}";; esac; done)
# HOT-ORDER-3 guard: every library function a probe's source names must have run in its trace. gcc
# folds or replaces libc calls with constant arguments (p_cold's strcoll at -O2), and the list then
# silently misses the function; call it through a volatile pointer instead.
nm --defined-only -g "$P/libc.a" 2>/dev/null | awk 'NF == 3 && $2 ~ /^[TWi]$/ {print $3}' | sort -u > "$work/libfns"
check_calls() { # <probe> <n>: after its trace (names.<n>)
    local missing n=$2
    # A fortified call (printf -> __printf_chk) and a header redirect (libintl.h at -O2: dgettext ->
    # __dcgettext) count as the call.
    { cat "$work/names.$n"; sed -En 's/^__(.*)_chk$/\1/p' "$work/names.$n"
      if grep -qx __dcgettext "$work/names.$n"; then printf '%s\n' gettext dgettext dcgettext; fi; } > "$work/ran"
    missing=$(gcc -fpreprocessed -E "$1.c" 2>/dev/null | grep -v '^#' | grep -oE '[A-Za-z_][A-Za-z0-9_]*' | sort -u |
              grep -Fx -f "$work/libfns" | grep -Fxv -f "$work/ran" | tr '\n' ' ' || true)
    if [ -n "$missing" ]; then echo "regen: $1: library functions named in the source never ran: $missing" >&2; exit 1; fi
}
# STARTUP-TEXT-WINDOWS (D-X.49) and LAYOUT-TEXT-PIN-9 (ruling #4539): page_faults.c linked as
# crates/core/tests/hot_text_products.rs links it; its `true` mode gives the start-up keys below, and
# its `locale` and `tool` modes (the setlocale + printf and coreutils-shaped programs whose faults
# that test bounds) are always traced, so a regen without extra programs keeps their keys.
cc -std=gnu17 -O1 -static -nostdlib -fno-builtin "$P/liella-crt1.o" "$T/crates/core/tests/oracle/page_faults.c" \
    "$P/libc.a" -lgcc -o page_faults
for p in $probes; do
    LIELLA_PRODUCTS=$P "$T/scripts/liella-cc" -O2 -o "$p" "$p.c"
    if [ "$p" = p_cat ]; then
        # HOT-ORDER-3: p_cat's read path here (empty stdin), its write path (fwrite) at the end of the
        # group below: fwrite ahead of the printf group cost the first snprintf one fault (Ivy Bridge host B:
        # cold.c snprintf 12 -> 13, cold2 first snprintf 8 -> 9).
        stdin=/dev/null trace stdio en_US.UTF-8 ./p_cat
    elif [ "$p" = p_fork ]; then
        # TIMING-COV-FORK-COW-FAULTS: trace into the fork child (its text windows fault after fork).
        follow=child trace fork en_US.UTF-8 ./p_fork
        checks="$checks p_fork:$n"
    elif [ "$p" = p_cold ]; then
        # LAYOUT-TEXT-PIN-3: p_cold's localtime_r/mktime in the zone the fault-bounded probes
        # (page_faults.c localtime, localtime_mktime, mktime) and cbench_opt's zone rows use, not the
        # host's: a UTC host traced no rule-string parse, no period cache and another crate's vector
        # growth, so those stayed out of the group and cost the first localtime_r two faults.
        tz=America/New_York trace "$(tier_of "$p")" en_US.UTF-8 "./$p"
        checks="$checks $p:$n"
    else
        trace "$(tier_of "$p")" en_US.UTF-8 "./$p"
        checks="$checks $p:$n"
    fi
done
trace locale C ./p_loc
if [ -f p_cat.c ]; then trace stdio en_US.UTF-8 ./p_cat; checks="$checks p_cat:$n"; fi
# The bound test runs these modes with LANG=en_US.UTF-8 and stdin /dev/null.
stdin=/dev/null trace locale en_US.UTF-8 ./page_faults locale
stdin=/dev/null trace tool en_US.UTF-8 ./page_faults tool
# LAYOUT-TRACE-AT-BUILD (LT-1): every other mode the bounds of hot_text_products.rs count, so a list traced
# from this build alone holds what they run. After `tool`: the first touches above keep their order and
# these add only what no earlier probe ran (tier fork for the child, extra for the rest).
stdin=/dev/null follow=child trace fork en_US.UTF-8 ./page_faults fork
for m in strings float mktime rebind; do stdin=/dev/null trace extra en_US.UTF-8 ./page_faults "$m"; done
# STARTUP-TEXT-WINDOWS (D-X.49): the functions `page_faults true` runs (the probe whose start-up faults
# crates/core/tests/hot_text_products.rs bounds, linked as that test links it), first-touch order, into
# $L/startup-text.keys; the layout check of layout/contracts.tsv counts their 64 KiB windows.
run_traced "$work/startup.names" "$work/gdb.startup.log" LANG=C HOT_TEXT_STDIN="$work/in.txt" HOT_TEXT_FOLLOW_FORK= -- ./page_faults true
# D-X.62: the probe-text modes, traced with the others (probe_text.sh merges them below).
probe_text_trace() { local rc=0; bash "$here/probe_text.sh" "$T" --products "$P" --trace-only "$work/probe" > "$work/probe.log" 2>&1 || rc=$?; echo $rc > "$work/probe.log.rc"; }
probe_text_trace &
traces_done
# LAYOUT-TRACE-AT-BUILD-2: the trace set covers every mode hot_text_products.rs measures, which is every
# mode of page_faults.c (its init_startup_data_accesses_are_in_the_data1_cluster checks that, parsing the
# source the same way; `true` is the default mode, selected by no strcmp): a mode with no trace above
# (trace ./page_faults <mode>), no start-up trace (true) and no probe_text.sh trace fails here by name,
# whatever its fault count happens to read on this build.
pmodes=$(grep -oE 'strcmp\(argv\[1\], "[A-Za-z0-9_]+"' "$T/crates/core/tests/oracle/page_faults.c" |
         sed -E 's/.*"([A-Za-z0-9_]+)"$/\1/' | grep -vx relay | sort -u || true)
if [ "$(printf '%s\n' "$pmodes" | grep -c .)" -lt 3 ]; then
    echo "regen: --all-modes: found only '$pmodes' as page_faults.c modes (strcmp(argv[1], \"<mode>\")): the parse no longer matches the source" >&2
    exit 1
fi
untraced=
for m in true $pmodes; do
    if [ "$m" = true ]; then [ -s "$work/startup.names" ] && continue
    else grep -qxF "./page_faults $m" "$work/traced" && continue; [ -s "$work/probe/names.$m" ] && continue; fi
    untraced="$untraced $m"
done
if [ -n "$untraced" ]; then
    echo "regen: --all-modes: the trace set misses page_faults mode(s):$untraced (hot_text_products.rs measures every mode of page_faults.c; trace each one here or in probe_text.sh)" >&2
    exit 1
fi
echo "regen: --all-modes: every page_faults mode traced: true $(printf '%s ' "$pmodes" | tr '\n' ' ')"
python3 -B "$here/keys.py" --tagged "startup=$work/startup.names" > "$work/startup.keys"
skeys=$L/startup-text.keys
{
    echo "# Written by tools/hottext/regen.sh during cargo xtask so (a trace of this build; never committed)."
    echo "# STARTUP-TEXT-WINDOWS (layout/contracts.tsv, D-X.49): the functions \`page_faults true\` runs (crates/core/tests/oracle/page_faults.c"
    echo "# linked as crates/core/tests/hot_text_products.rs links it): build-stable keys (tools/hottext/keys.py), first-touch order."
    echo "# <key> TAB startup TAB <profile traced>."
    python3 -B "$here/order.py" "$work/startup.keys" "$profile"
} > "$work/startup.ordered"
# shellcheck disable=SC2046
python3 -B "$here/keys.py" --tagged $(cat "$work/tagged") > "$work/hit.keys"
# Only names that are function sections of libc.a (drops the programs' own functions).
# (The traced archive is ungrouped, but its sections may still carry a hot or cold prefix: `.text.sorted.h<n>.`,
# `.text.hot.`, `.text.unlikely.<f>` of a #[cold] function, `.text.liella_cold.<f>.liella_cold` (D-X.63);
# keys.py --sections strips every one of them, LAYOUT-TEXT-PIN-9 #4541.)
readelf -SW "$P/libc.a" > "$work/readelf"
python3 -B "$here/keys.py" --sections "$work/readelf" > "$work/sections"
python3 -B "$here/keys.py" "$work/sections" > "$work/lib.keys"
# LAYOUT-TEXT-PIN-6 (#4467): labels inside a function section (assembly entry labels, aliases) -> its key,
# so a start-up key traced at such a label brings its section to the start-up head.
objdump -t "$P/libc.a" > "$work/syms"
python3 -B "$here/keys.py" --aliases "$work/syms" > "$work/alias.keys"
awk -F'\t' 'NR == FNR { lib[$1]; next } $1 in lib' "$work/lib.keys" "$work/hit.keys" > "$work/new.keys"  # keeps the touch order
{
    echo "# Functions grouped as hot text in libc.a (PAGEFAULTS-2), in this order (HOT-ORDER: first touch): build-stable keys, see tools/hottext/keys.py."
    echo "# Written by tools/hottext/regen.sh during cargo xtask so (a trace of this build; never committed)."
    echo "# <key> TAB <tier> TAB <profile traced> (HOTTEXT-STARTUP-COMPLETE; tiers startup fork stdio strings locale tool extra)."
    # Ruling #4417 (D-X.63): every STARTUP-TEXT-WINDOWS key joins the start-up tier at the head.
    python3 -B "$here/order.py" "$work/new.keys" "$profile" --startup-head "$work/startup.ordered" --alias "$work/alias.keys"
} > "$work/list.ordered"
mv "$work/startup.ordered" "$skeys"
echo "startup-text: $(wc -l < "$work/startup.keys") keys traced ($profile), $(grep -vc '^#' "$skeys") in $skeys"
mv "$work/list.ordered" "$list"
echo "hot-text: $(wc -l < "$work/new.keys") keys traced ($profile), $(grep -vc '^#' "$list") in $list"
# D-X.62: the functions of the other fault-bounded probe modes that no hot-text tier covers, into
# $L/probe-text.keys (the start-up read-only group takes their read-only data).
bash "$here/probe_text.sh" "$T" --products "$P" --merge-only "$work/probe"
# STARTUP-DATA-WINDOWS (D-X.50): the data objects the same probe touches, into $L/startup-data.keys
# (it reads the startup-text.keys written above).
STARTUP_DATA_PRODUCTS=$P bash "$here/startup_data.sh" "$T"
