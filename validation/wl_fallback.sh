# LAB-ONLY: this script drives the project's own test hosts and is not needed to build or test Liella.
# Sourced by wl_count.sh / wl_percall.sh / wl_alloctrace.sh for the path taken when S is unset or not exported (WLC-FALLBACK,
# 2026-10-11). Needs: keke (the ssh command), label, and the caller's nowait.
# Why this file: the old fallback ran  bash -lc '$cmd; echo $? > done'  and $cmd itself holds single quotes
# (echo 'wl_base: ...' && WL_PROGS='...'), which closed the outer quote: the keke job died after "wl_base:" (9-byte log, no
# .done) and the caller looped  until test -f .done; sleep 20  forever (one agent lost 55 min). Now the command travels
# base64-encoded into a script file (no quoting at all), and the wait gives up when the log stops growing.
# WLFB_STALL_MIN (10) = minutes without log growth (and no .done) before wait gives up with exit 4.
wl_fb_warn() {   # <tool>
    echo "WARNING: $1: S is unset/not exported (or its tools/vm_run.sh missing): the queued vm_run path is SKIPPED; running directly on keke without the queue (export S=<scratchpad>)" >&2
}
wl_fb_start() {   # runs $cmd detached on keke; log ~/logs/$label.log, exit status ~/logs/$label.done
    local b64
    b64=$(printf '%s\necho $? > ~/logs/%s.done\n' "$cmd" "$label" | base64 | tr -d '\n') || return 1
    $keke "mkdir -p ~/logs && echo $b64 | base64 -d > ~/logs/$label.sh && (trap '' HUP; setsid nohup bash -l ~/logs/$label.sh > ~/logs/$label.log 2>&1 &) </dev/null"
}
wl_fb_reattach() {
    echo "re-attach: ssh claude@10.0.100.53 'tail -f ~/logs/$label.log'   (done file: keke:~/logs/$label.done, script ~/logs/$label.sh)"
}
wl_fb_wait() {   # returns 0 when .done exists, 4 when the log stalled
    local sz last=-1 lastt now max=$(( ${WLFB_STALL_MIN:-10} * 60 ))
    lastt=$(date +%s)
    while :; do
        sz=$($keke "if test -f ~/logs/$label.done; then echo DONE; else stat -c %s ~/logs/$label.log 2>/dev/null || echo 0; fi") || sz=
        [ "$sz" = DONE ] && return 0
        now=$(date +%s)
        if [ -n "$sz" ] && [ "$sz" != "$last" ]; then last=$sz; lastt=$now; fi   # ssh failure ("") counts as no growth
        if [ $(( now - lastt )) -ge "$max" ]; then
            echo "wl_fallback: GAVE UP: keke:~/logs/$label.log stopped growing ($last bytes) for ${WLFB_STALL_MIN:-10} min and there is no .done; the job died or hangs (script ~/logs/$label.sh)" >&2
            $keke "tail -n 20 ~/logs/$label.log" >&2
            return 4
        fi
        sleep 20
    done
}
