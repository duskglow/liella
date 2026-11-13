#!/bin/bash
# Regenerate the hot-text list (PAGEFAULTS-2, crates/libc-so/hot-text.list) on a Linux host with gdb.
# usage: tools/hottext/regen.sh <tree with built products> [--replace] [--build] [--allow-drop] ['<static program> <args>'...]
#   Traces the startup / stdio / setlocale+printf probes, the `locale` and `tool` modes of
#   crates/core/tests/oracle/page_faults.c (always: LAYOUT-TEXT-PIN-9), and any extra static programs linked
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
#   LAYOUT-TEXT-PIN-9 (ruling #4539): a key of the old list or startup-text.keys that this profile's
#   trace no longer hits and no other profile keeps is named on stderr, and the run then writes
#   nothing unless --allow-drop is given (a key is never dropped silently).
#   HOTTEXT-REGEN-UNBLOCK: --build (needs <tree> = this script's tree) builds the products itself, in the
#   profile LIELLA_FULL_PROFILE selects, with `cargo xtask so --hottext-regen`: the dead-key check is
#   reported, not enforced, so a stale key no longer blocks its own repair. After rewriting the list it
#   rebuilds with plain `cargo xtask so`, which must pass the check. Nothing else may pass that flag
#   (`cargo xtask static`). Both profiles: `regen.sh . --build` then `LIELLA_FULL_PROFILE=1 regen.sh . --build`.
#   HOTLIST-REGEN-CHEAP: the traces of one run go in parallel (HOTTEXT_JOBS at once, default nproc), and
#   each trace keeps its number, so the merge reads them in the order above. --both (needs --build's tree)
#   is the cheap regen of both profiles: `cargo xtask so --hottext-regen --libc-only` builds only what the
#   traces link (target/product/regen/<profile>), each profile is traced (--trace-only) and merged
#   (--merge-only), fast then release, the release products built from the lists the fast merge wrote (as
#   the one-profile path builds them), and `cargo xtask hottext-recheck` reruns only the grouping and its
#   dead-key check. A list is rewritten only when its bytes change (runner.sh put_changed).
#   --products <dir>: trace the products in <dir> (default <tree>/target/liella).
set -eu
here=$(cd "$(dirname "$0")" && pwd)
T=$(cd "$1" && pwd); shift
# HOTLIST-TRAIN-REGEN: the generated lists are read and written in LIELLA_HOT_LISTS when it is set
# (`cargo xtask hot-lists` sets it: target/hot-lists, or crates/libc-so in place); the builds this
# script starts inherit it, so they read the lists being rewritten. The hand-written inputs stay in crates/libc-so.
L=${LIELLA_HOT_LISTS:-$here/../../crates/libc-so}
list=$L/hot-text.list
replace=0
build=0
allow_drop=0
both=0
P=$T/target/liella
phase=all
pdir=
while [ $# -gt 0 ]; do
    case $1 in --replace) replace=1; shift;; --build) build=1; shift;; --allow-drop) allow_drop=1; shift;; --both) both=1; shift;;
        --products) P=$(cd "$2" && pwd); shift 2;; --trace-only) phase=trace; pdir=$2; shift 2;; --merge-only) phase=merge; pdir=$2; shift 2;;
        *) break;; esac
done
own=$(cd "$here/../.." && pwd)
if [ $both = 1 ]; then
    if [ "$T" != "$own" ] || [ $build = 1 ] || [ $replace = 1 ] || [ $# -gt 0 ]; then echo "regen: --both regenerates its own tree ($own), merge mode, no extra programs" >&2; exit 1; fi
    ad=; [ $allow_drop = 1 ] && ad=--allow-drop
    R=$T/target/product/regen
    rm -rf "$R"; mkdir -p "$R"
    t0=$(date +%s); tp=$t0
    lap() { local t; t=$(date +%s); echo "regen-both: phase $1: $((t - tp)) s (total $((t - t0)) s)"; tp=$t; }
    inputs="hot-text.list startup-text.keys probe-text.keys startup-data.keys"
    mkdir -p "$R/inputs"; for f in $inputs; do cp "$L/$f" "$R/inputs/$f"; done
    (cd "$T" && cargo xtask so --hottext-regen --libc-only "$R/fast") || { echo "regen: fast product build failed" >&2; exit 1; }
    lap build-fast
    bash "$0" "$T" --products "$R/fast" --trace-only "$R/fast.trace" > "$R/fast.trace.log" 2>&1 || { tail -5 "$R/fast.trace.log" >&2; exit 1; }
    lap traces-fast
    # shellcheck disable=SC2086
    bash "$0" "$T" --products "$R/fast" --merge-only "$R/fast.trace" $ad
    lap merge-fast
    # HOTLIST-REGEN-CHEAP-2 (#5093): the release products are built from the lists the fast merge wrote, as
    # the one-profile path builds them. Not ahead of the merge: merge.py puts the new trace's first-touch
    # order first, the fast and release orders differ, so even a regen that ends where it started reorders
    # the lists in the fast merge (the release merge puts the release order back), and a release build
    # made before it was never the one traced.
    changed=; for f in $inputs; do cmp -s "$L/$f" "$R/inputs/$f" || changed="$changed $f"; done
    echo "regen-both: the fast merge changed:${changed:- nothing}"
    (cd "$T" && LIELLA_FULL_PROFILE=1 cargo xtask so --hottext-regen --libc-only "$R/release") || { echo "regen: release product build failed" >&2; exit 1; }
    lap build-release
    bash "$0" "$T" --products "$R/release" --trace-only "$R/release.trace" > "$R/release.trace.log" 2>&1 || { tail -5 "$R/release.trace.log" >&2; exit 1; }
    lap traces-release
    # shellcheck disable=SC2086
    bash "$0" "$T" --products "$R/release" --merge-only "$R/release.trace" $ad
    lap merge-release
    (cd "$T" && cargo xtask hottext-recheck) || { echo "regen: the rewritten list fails the grouping check" >&2; exit 1; }
    lap recheck
    exit 0
fi
if [ $build = 1 ]; then
    if [ "$T" != "$own" ]; then echo "regen: --build builds the list's own tree; give $own" >&2; exit 1; fi
    (cd "$T" && cargo xtask so --hottext-regen) || { echo "regen: product build failed" >&2; exit 1; }
fi
profile=$(cat "$P/.profile" 2>/dev/null || true)
case $profile in fast|release) ;; *) echo "regen: $P/.profile: want fast or release, got '$profile' (build the products first)" >&2; exit 1;; esac
if [ $phase = all ]; then
    work=$(mktemp -d "${TMPDIR:-/tmp}/hottext.XXXXXX")
    trap 'rm -rf "$work"' EXIT
elif [ $phase = trace ]; then
    mkdir "$pdir"; work=$(cd "$pdir" && pwd)
else
    work=$(cd "$pdir" && pwd)
fi
cd "$work"
n=0
# shellcheck source=tools/hottext/runner.sh
. "$here/runner.sh"
if [ $phase != merge ]; then
cp "$here"/probes/*.c "$work"/
echo x > "$work/in.txt"
tier_of() { # <probe> -> its tier (HOTTEXT-STARTUP-COMPLETE)
    case $1 in
        p_true) echo startup;; p_fork) echo fork;; p_puts|p_cat|p_fmt) echo stdio;; p_str) echo strings;;
        p_loc) echo locale;; p_locuse|p_nss) echo tool;; *) echo extra;;
    esac
}
: > "$work/tagged"
trace() { # <tier> <LANG> <program> <args...>
    local tier=$1 lang=$2; shift 2
    trace_tagged "$tier" LANG="$lang" LC_ALL= HOT_TEXT_STDIN="${stdin:-$work/in.txt}" HOT_TEXT_FOLLOW_FORK="${follow:-}" -- "$@"
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
        TZ=America/New_York trace "$(tier_of "$p")" en_US.UTF-8 "./$p"
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
for cmd in "$@"; do
    # shellcheck disable=SC2086
    trace tool en_US.UTF-8 $cmd
done
# STARTUP-TEXT-WINDOWS (D-X.49): the functions `page_faults true` runs (the probe whose start-up faults
# crates/core/tests/hot_text_products.rs bounds, linked as that test links it), first-touch order, into
# crates/libc-so/startup-text.keys; the layout check of layout/contracts.tsv counts their 64 KiB windows.
run_traced "$work/startup.names" "$work/gdb.startup.log" LANG=C LC_ALL= HOT_TEXT_STDIN="$work/in.txt" HOT_TEXT_FOLLOW_FORK= -- ./page_faults true
# D-X.62: the probe-text modes, traced with the others (probe_text.sh merges them below).
probe_text_trace() { local rc=0; bash "$here/probe_text.sh" "$T" --products "$P" --trace-only "$work/probe" > "$work/probe.log" 2>&1 || rc=$?; echo $rc > "$work/probe.log.rc"; }
probe_text_trace &
traces_done
fi  # trace phase
[ $phase = trace ] && exit 0
python3 "$here/keys.py" --tagged "startup=$work/startup.names" > "$work/startup.keys"
skeys=$L/startup-text.keys
if [ $replace = 1 ] || [ ! -f "$skeys" ]; then : > "$work/startup.old"; else grep -v '^#' "$skeys" > "$work/startup.old" || true; fi
{
    echo "# @generated by tools/hottext/regen.sh; do not edit by hand."
    echo "# STARTUP-TEXT-WINDOWS (layout/contracts.tsv, D-X.49): the functions \`page_faults true\` runs (crates/core/tests/oracle/page_faults.c"
    echo "# linked as crates/core/tests/hot_text_products.rs links it): build-stable keys (tools/hottext/keys.py), first-touch order."
    echo "# <key> TAB startup TAB <profiles traced in>."
    python3 "$here/merge.py" "$work/startup.old" "$work/startup.keys" "$profile" --dropped "$work/startup.dropped"
} > "$work/startup.merged"
# shellcheck disable=SC2046
python3 "$here/keys.py" --tagged $(cat "$work/tagged") > "$work/hit.keys"
# Only names that are function sections of libc.a (drops the programs' own functions).
# (A grouped archive's sections carry the hot prefix, `.text.sorted.h<n>.` or the older `.text.hot.`;
# a #[cold] function's section is `.text.unlikely.<f>`, HOTTEXT-COLD-STARTUP; an unlisted one the build
# renamed `.text.liella_cold.<f>.liella_cold`, D-X.63: keys.py --sections strips every one of them,
# LAYOUT-TEXT-PIN-9 #4541, so a function renamed cold re-enters the list when a trace hits it.)
readelf -SW "$P/libc.a" > "$work/readelf"
python3 "$here/keys.py" --sections "$work/readelf" > "$work/sections"
python3 "$here/keys.py" "$work/sections" > "$work/lib.keys"
# LAYOUT-TEXT-PIN-6 (#4467): labels inside a function section (assembly entry labels, aliases) -> its key,
# so a start-up key traced at such a label brings its section to the start-up head.
objdump -t "$P/libc.a" > "$work/syms"
python3 "$here/keys.py" --aliases "$work/syms" > "$work/alias.keys"
awk -F'\t' 'NR == FNR { lib[$1]; next } $1 in lib' "$work/lib.keys" "$work/hit.keys" > "$work/new.keys"  # keeps the touch order
if [ $replace = 1 ] || [ ! -f "$list" ]; then : > "$work/old.keys"; else grep -v '^#' "$list" > "$work/old.keys" || true; fi
{
    echo "# Functions grouped as hot text in libc.a (PAGEFAULTS-2), in this order (HOT-ORDER: first touch): build-stable keys, see tools/hottext/keys.py."
    echo "# Generated by tools/hottext/regen.sh; do not edit by hand."
    echo "# <key> TAB <tier> TAB <profiles traced in> (HOTTEXT-STARTUP-COMPLETE; tiers startup fork stdio strings locale tool extra)."
    # Ruling #4417 (D-X.63): every STARTUP-TEXT-WINDOWS key (merged above) joins the start-up tier at the head.
    python3 "$here/merge.py" "$work/old.keys" "$work/new.keys" "$profile" --startup-head "$work/startup.merged" --alias "$work/alias.keys" \
        --dropped "$work/list.dropped"
} > "$work/list.merged"
# LAYOUT-TEXT-PIN-9 (ruling #4539): a key that was on either file and that this trace drops is named here,
# and without --allow-drop nothing is written: a short trace (a probe that stopped running a path) must
# never shrink the list silently. Pass --allow-drop once each named function is really gone (renamed,
# inlined, removed) from this profile.
for d in startup list; do
    if [ -s "$work/$d.dropped" ]; then
        echo "regen: $d: $(wc -l < "$work/$d.dropped") key(s) on the old file that no $profile trace hit:" >&2
        sed 's/^/regen:   DROPPED /' "$work/$d.dropped" >&2
    fi
done
if [ -s "$work/startup.dropped" ] || [ -s "$work/list.dropped" ]; then
    if [ $allow_drop = 0 ]; then
        echo "regen: refusing to drop them (nothing written); rerun with --allow-drop if each one is really gone" >&2
        exit 1
    fi
    echo "regen: --allow-drop: dropping them" >&2
fi
put_changed "$work/startup.merged" "$skeys"
echo "startup-text: $(wc -l < "$work/startup.keys") keys traced ($profile), $(grep -vc '^#' "$skeys") in $skeys"
put_changed "$work/list.merged" "$list"
echo "hot-text: $(wc -l < "$work/new.keys") keys traced ($profile), $(grep -vc '^#' "$list") in $list"
# D-X.62: the functions of the other fault-bounded probe modes that no hot-text tier covers, into
# crates/libc-so/probe-text.keys (the start-up read-only group takes their read-only data).
pr=; [ $replace = 1 ] && pr=--replace
# shellcheck disable=SC2086
bash "$here/probe_text.sh" "$T" --products "$P" --merge-only "$work/probe" $pr
# STARTUP-DATA-WINDOWS (D-X.50): the data objects the same probe touches, into crates/libc-so/startup-data.keys
# (it reads the startup-text.keys written above).
# shellcheck disable=SC2086
STARTUP_DATA_PRODUCTS=$P bash "$here/startup_data.sh" "$T" $pr
if [ $build = 1 ]; then
    # The rewritten list must pass the dead-key check in this profile (merge.py drops the keys this
    # trace no longer hits from this profile).
    (cd "$T" && cargo xtask so) || { echo "regen: the rewritten list still fails cargo xtask so ($profile)" >&2; exit 1; }
fi
