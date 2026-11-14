#!/bin/bash
# Trace the functions of the fault-bounded probe modes that no hot-text tier covers (D-X.62) into
# $LIELLA_HOT_LISTS/probe-text.keys (the lists of one product build, LT-5: never committed).
# usage: LIELLA_HOT_LISTS=<dir> tools/hottext/probe_text.sh <tree with built products> [--products <dir>]
#   crates/core/tests/hot_text_products.rs bounds the page faults of `page_faults <mode>` for several
#   modes. The start-up read-only group (xtask/src/startup_ro.rs) takes the read-only data of the
#   functions those modes run: the hot-text tiers cover `true`, `locale` and the tool (HOT_TIERS there);
#   the modes in MODES below run functions the hot-text list files under `extra` (with every other
#   cold traced function), so they are traced here, each on its own, as the test runs them (env LANG,
#   linked as the test links page_faults.c). Output `<key>\textra\t<profile>`, first-touch order, of
#   this build's trace only (order.py; regen.sh, run by cargo xtask so, calls this).
#   A new fault-bounded mode whose functions sit in no HOT_TIERS tier goes into MODES.
set -eu
here=$(cd "$(dirname "$0")" && pwd)
T=$(cd "$1" && pwd); shift
#   HOTLIST-REGEN-CHEAP: --products <dir> (default <tree>/target/liella); the modes are traced in parallel;
#   --trace-only <new dir> traces into <dir> and stops, --merge-only <dir> merges a trace made so (regen.sh).
P=$T/target/liella
phase=all
while [ $# -gt 0 ]; do
    case $1 in --products) P=$(cd "$2" && pwd); shift 2;;
        --trace-only) phase=trace; pdir=$2; shift 2;; --merge-only) phase=merge; pdir=$2; shift 2;;
        *) echo "probe_text: unknown argument $1" >&2; exit 1;; esac
done
MODES="localtime localtime_mktime"
profile=$(cat "$P/.profile" 2>/dev/null || true)
case $profile in fast|release) ;; *) echo "probe_text: $P/.profile: want fast or release, got '$profile'" >&2; exit 1;; esac
if [ $phase = all ]; then work=$(mktemp -d "${TMPDIR:-/tmp}/probetext.XXXXXX"); trap 'rm -rf "$work"' EXIT
elif [ $phase = trace ]; then mkdir "$pdir"; work=$(cd "$pdir" && pwd); else work=$(cd "$pdir" && pwd); fi
# shellcheck source=tools/hottext/runner.sh
. "$here/runner.sh"
tagged=
for m in $MODES; do tagged="$tagged extra=$work/names.$m"; done  # these functions' tier is extra
if [ $phase != merge ]; then
cc -std=gnu17 -O1 -static -nostdlib -fno-builtin "$P/liella-crt1.o" "$T/crates/core/tests/oracle/page_faults.c" \
    "$P/libc.a" -lgcc -o "$work/page_faults"
echo x > "$work/in.txt"
canon_prepare  # LAYOUT-TRACE-HOST-CANONICAL: the tree's host data (canon_host.sh)
for m in $MODES; do
    run_traced "$work/names.$m" "$work/gdb.$m.log" LANG=en_US.UTF-8 HOT_TEXT_STDIN="$work/in.txt" HOT_TEXT_FOLLOW_FORK= -- "$work/page_faults" "$m"
done
traces_wait || exit 1
fi
[ $phase = trace ] && exit 0
# shellcheck disable=SC2086
python3 -B "$here/keys.py" --tagged $tagged > "$work/probe.keys"
L=${LIELLA_HOT_LISTS:-}
if [ -z "$L" ] || [ ! -d "$L" ]; then
    echo "probe_text: LIELLA_HOT_LISTS must name the lists directory of this build (cargo xtask so sets it; no list is committed, LT-5)" >&2
    exit 1
fi
pkeys=$L/probe-text.keys
{
    echo "# Written by tools/hottext/probe_text.sh during cargo xtask so (a trace of this build; never committed)."
    echo "# D-X.62: the functions the fault-bounded \`page_faults <mode>\` probes of crates/core/tests/hot_text_products.rs run that"
    echo "# no hot-text tier covers (modes: $MODES): build-stable keys (tools/hottext/keys.py), first-touch order."
    echo "# <key> TAB extra TAB <profile traced>."
    python3 -B "$here/order.py" "$work/probe.keys" "$profile"
} > "$pkeys.tmp"
put_changed "$pkeys.tmp" "$pkeys"
echo "probe-text: $(wc -l < "$work/probe.keys") keys traced ($profile), $(grep -vc '^#' "$pkeys") in $pkeys"
