# LAB-ONLY: this script drives the project's own test hosts and is not needed to build or test Liella.
# Sourced by wl_percall.sh and wl_count.sh for --nowait (TOOLS-24, 2026-10-11): print the label at once and do the uploads
# and the queueing in a detached local process. Before, --nowait blocked for 5+ minutes on a loaded keke (ssh round trips
# of 0.3-14 s each, ~43 MB per new sha) before it printed anything, and three agents lost their last step to it.
# Needs: label, S, and the caller's functions prep (uploads; sets cmd) and queue (VM_NOWAIT vm_run). The launch log is
# $S/work/launch/<label>.log; its last line is "wl_launch: LAUNCHED <label>" or "wl_launch: LAUNCH FAILED (exit N) <label>".
# vm_wait.sh knows this log: on a label that is still launching it waits for the launch instead of saying NO SUCH JOB, and a
# failed launch is reported with the log tail. The spec file (sfile, wl_percall) is copied first, so an edit after the
# launch cannot change what runs. WL_LAUNCH_SYNC=1 runs the launch in the foreground (the old behaviour, same output).
wl_launch_detached() {
    local ld="$S/work/launch" lg
    mkdir -p "$ld" || return 1
    lg="$ld/$label.log"
    if [ -n "${sfile:-}" ]; then cp "$sfile" "$ld/$label.spec.c" || return 1; sfile="$ld/$label.spec.c"; fi
    if [ "${WL_LAUNCH_SYNC:-0}" = 1 ]; then
        prep || return 1
        queue || return 2
    else
        (
            trap '' HUP
            echo "wl_launch: $label started $(date '+%F %T') pid $BASHPID"
            prep; rc=$?
            [ $rc = 0 ] && { queue; rc=$?; }
            if [ $rc = 0 ]; then echo "wl_launch: LAUNCHED $label $(date '+%F %T')"
            else echo "wl_launch: LAUNCH FAILED (exit $rc) $label $(date '+%F %T')"; fi
            exit $rc
        ) > "$lg" 2>&1 < /dev/null &
        disown 2>/dev/null
        echo "launching $label in the background (uploads + queue; log $lg)"
    fi
    echo "re-attach: bash \$S/tools/vm_wait.sh keke $label tail   (log keke:~/logs/$label.log)"
    return 0
}
