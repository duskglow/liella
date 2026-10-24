#!/bin/sh
# Stress runs of conformance programs (implementation-plan.md, phase 6 exit: 1,000 runs without
# failure under `taskset` on 1 and N cores).
#
#   sh conformance/stress.sh RUNS CPUS name ...
#
# CPUS is a `taskset -c` list (`0` for one core, `0-$(($(nproc)-1))` for all). Each program is
# built once by run.sh (which also checks it), then run RUNS times with its directives
# (RUN-ARGS, RUN-WRAP, EXPECT-EXIT, EXPECT-SIGNAL). Programs with RUN-ENV or RUN-SECURE are
# not supported here. Prints one line per program and fails on the first bad run.
set -eu
here=$(cd "$(dirname "$0")" && pwd)
runs=$1
cpus=$2
shift 2
sh "$here/run.sh" "$@" >/dev/null
directive() { sed -n "s|^/\* $1: \(.*\) \*/\$|\1|p" "$2"; }
for name in "$@"; do
    src="$here/c/$name.c"
    exe="$here/build/$name"
    if [ -n "$(directive RUN-ENV "$src")$(directive RUN-SECURE "$src")" ]; then
        echo "stress: $name: RUN-ENV/RUN-SECURE not supported" >&2
        exit 2
    fi
    args=$(directive RUN-ARGS "$src")
    wrap=$(directive RUN-WRAP "$src")
    want_exit=$(directive EXPECT-EXIT "$src"); want_exit=${want_exit:-0}
    want_sig=$(directive EXPECT-SIGNAL "$src")
    [ -n "$want_sig" ] && want=$((128 + want_sig)) || want=$want_exit
    i=0
    while [ "$i" -lt "$runs" ]; do
        set +e
        # shellcheck disable=SC2086
        taskset -c "$cpus" $wrap "$exe" $args >/dev/null 2>"$here/build/$name.stress.stderr"
        rc=$?
        set -e
        if [ "$rc" -ne "$want" ]; then
            echo "stress: FAIL $name run $((i + 1)) on cpus $cpus: status $rc, expected $want"
            exit 1
        fi
        i=$((i + 1))
    done
    echo "stress: $name: $runs runs on cpus $cpus"
done
