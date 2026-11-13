#!/bin/bash
# LAB-ONLY: drives validation/libm_tail/probe.c (LIBM-TAIL-PROBE; no spec clause): per-input instruction cost of
# libm functions, Liella vs the installed glibc (black box), run under jobrun on a build host.
#
#   bash validation/libm_tail/run.sh <liella-tree> <outdir> <spec>...
#   spec = mode:func:S (probe.c modes u32 b32 u64 b64 n64; S = stride or samples per block), e.g. u32:expf:1
#   (every f32 input). TAIL_PAR (default 4) processes per function and library, each a residue class of the
#   blocks. A spec whose .done marker exists is skipped (a rerun resumes). n64 reads env TAIL_POINTS.
# Binaries: <outdir>/probe-glibc (gcc -O2 -z now, dynamic libm) and <outdir>/probe-liella (scripts/liella-cc,
# static, the tree's target/liella products), built once per outdir.
set -u
tree=$1 out=$2
shift 2
par=${TAIL_PAR:-4}
src=$(cd "$(dirname "$0")" && pwd)/probe.c
mkdir -p "$out"
if [ ! -x "$out/probe-glibc" ]; then
    gcc -O2 -Wl,-z,now -o "$out/probe-glibc.$$" "$src" -lm || exit 1
    mv -f "$out/probe-glibc.$$" "$out/probe-glibc"
fi
if [ ! -x "$out/probe-liella" ]; then
    LIELLA_PRODUCTS=$tree/target/liella "$tree/scripts/liella-cc" -O2 -o "$out/probe-liella.$$" "$src" -lm || exit 1
    mv -f "$out/probe-liella.$$" "$out/probe-liella"
fi
rc=0
for spec in "$@"; do
    mode=${spec%%:*} rest=${spec#*:}
    func=${rest%%:*} S=${rest#*:}
    # mode n64 takes a 4th field: the points, separated by '/'
    case $S in */*|*:*) pts=${S#*:}; S=${S%%:*}; export TAIL_POINTS=${pts//\//,} ;; esac
    for lib in glibc liella; do
        tag=$func.$mode.$S.$lib
        [ -f "$out/$tag.done" ] && continue
        t0=$(date +%s)
        pids=()
        for ((p = 0; p < par; p++)); do
            "$out/probe-$lib" "$func" "$mode" "$out/$tag" "$p" "$par" "$S" &
            pids+=($!)
        done
        bad=0
        for pid in "${pids[@]}"; do wait "$pid" || bad=1; done
        if [ $bad = 0 ]; then
            echo "$(($(date +%s) - t0))" >"$out/$tag.done"
            echo "libm_tail: $tag done in $(($(date +%s) - t0)) s"
        else
            echo "libm_tail: $tag FAILED" >&2
            rc=1
        fi
    done
done
exit $rc
