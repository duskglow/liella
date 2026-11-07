#!/bin/bash
# LIBM-RETHINK step 2: per-function split of ONE call (Liella only) for representative libm rows, to see which
# tier runs per call. Uses the products wl_percall cached for this role's revision.
set -u
cd ~/w/r5 || exit 2
spec=~/work/libm-rethink/libm_rethink.c
rc=0
for cf in expf:expf exp:exp exp_upward:exp log:log sinl:sinl atanl:atanl sqrtl:sqrtl expl:expl \
          expf128:expf128 sinf128:sinf128 cexpf:cexpf cexp:cexp csqrt:csqrt; do
    c=${cf%%:*}; f=${cf#*:}
    echo "=== TRACE case=$c function=$f"
    bash validation/wl_trace.sh --spec "$spec" --max 600000 . "$c" "$f" || { echo "TRACE-FAILED $c rc=$?"; rc=1; }
done
exit $rc
