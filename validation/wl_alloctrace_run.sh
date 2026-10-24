#!/bin/bash
# LAB-ONLY: this script drives the project's own test hosts and is not needed to build or test Liella.
# wl_alloctrace.sh, keke side: run ONE workload's kit program under gdb and print the allocation summary.
#
#   wl_alloctrace_run.sh SHA WORKLOAD PROG TOP
#
# The workload command (wl_workloads.sh) runs unchanged except that @/PROG is a wrapper: its FIRST invocation runs the
# kit binary DIRECTLY under `gdb -batch -x wl_alloctrace.py` (later invocations, e.g. in a loop of 10, run untraced).
# Kit: ~/wl/kit/SHA/bin (made by wl_prep.sh). Output: the summary of wl_alloctrace.py, demangled by rustfilt if present.
set -u
W=${WL:-$HOME/wl}
here=$(cd "$(dirname "$0")" && pwd)
sha=$1 want=$2 prog=$3 top=$4
. "$here/wl_workloads.sh"
cmd=
for d in "${WL_DEFS[@]}"; do [ "${d%%|*}" = "$want" ] && cmd=${d#*|}; done
[ -n "$cmd" ] || { echo "wl_alloctrace: unknown workload $want"; exit 2; }
kit=$W/kit/$sha/bin
if [ -z "$prog" ]; then prog=$(echo "$cmd" | grep -o '@/[a-z0-9]*' | head -1 | cut -c3-); fi
[ -x "$kit/$prog" ] || { echo "wl_alloctrace: $kit/$prog missing (programs: $(ls "$kit" | tr '\n' ' '))"; exit 2; }
command -v gdb > /dev/null || { echo "wl_alloctrace: no gdb on this host"; exit 2; }
tmp=$W/tmp.alloctrace.$$
mkdir -p "$tmp/bin"
trap 'rm -rf "$tmp"' EXIT
cp "$here/wl_alloctrace.py" "$tmp/tr.py"
printf '%s\n' '#!/bin/sh' \
    "if mkdir '$tmp/once' 2>/dev/null; then" \
    "  TROUT='$tmp/summary' TRTOP='$top' TRSECS='${TRSECS:-240}' TRMIN='${TRMIN:-256}' exec timeout 1200 gdb -q -batch -ex 'set startup-with-shell off' -ex 'set confirm off' -x '$tmp/tr.py' --args '$kit/$prog' \"\$@\"" \
    "fi" "exec '$kit/$prog' \"\$@\"" > "$tmp/bin/$prog"
chmod +x "$tmp/bin/$prog"
c=${cmd//@\/$prog/$tmp/bin/$prog}
c=${c//@/$kit}
echo "workload $want, program $prog traced on its first invocation, kit $sha"
(cd "$W/work" && sh -c "$c" > /dev/null 2> "$tmp/err")
[ -f "$tmp/summary" ] || { echo "wl_alloctrace: no summary written; stderr tail:"; tail -n 15 "$tmp/err"; exit 1; }
if command -v rustfilt > /dev/null 2>&1; then rustfilt < "$tmp/summary"; elif [ -x "$HOME/.cargo/bin/rustfilt" ]; then "$HOME/.cargo/bin/rustfilt" < "$tmp/summary"; else cat "$tmp/summary"; fi
