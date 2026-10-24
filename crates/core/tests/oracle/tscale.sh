# Load-tolerant timeouts for the oracle run scripts (task FL-9): `. tscale.sh` sets TSCALE; use
# `timeout $((600 * TSCALE))`. Same formula as oracle/tscale.h and support/timeout.rs; keep them in step:
#   pressure = max(load1, running tasks - 1); scale = clamp(1 + ceil(pressure / cpus), 2, 10)
# LIELLA_TEST_TIMEOUT_SCALE=<n> overrides it.
TSCALE=${LIELLA_TEST_TIMEOUT_SCALE:-}
case $TSCALE in
'' | *[!0-9]* | 0)
    p=0
    if read -r l1 _ _ rt _ </proc/loadavg; then
        whole=${l1%%.*}
        frac=${l1#*.}00
        frac=${frac%"${frac#??}"}
        case $frac in 0?) frac=${frac#0} ;; esac
        p=$((whole * 100 + frac))
        r=$(((${rt%%/*} - 1) * 100))
        [ "$r" -gt "$p" ] && p=$r
    fi
    cpus=$(getconf _NPROCESSORS_ONLN 2>/dev/null || echo 1)
    [ "$cpus" -ge 1 ] 2>/dev/null || cpus=1
    TSCALE=$((1 + (p + 100 * cpus - 1) / (100 * cpus)))
    [ "$TSCALE" -lt 2 ] && TSCALE=2
    [ "$TSCALE" -gt 10 ] && TSCALE=10
    ;;
esac
: # a sourced file ends with a succeeding command (the tests above may fail under set -e)
