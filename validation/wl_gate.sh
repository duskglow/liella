#!/bin/bash
# LAB-ONLY: this script drives the project's own test hosts and is not needed to build or test Liella.
# THE WORKLOAD GATE: real programs (coreutils, sed, bash, grep, gawk, diffutils, tar, gzip,
# make, built statically against Liella) timed against the same programs on glibc, for a
# Liella commit and a baseline commit (INVIOLABLE rule of the owner, 2026-10-09: no real
# workload may get slower overall).
#
#   validation/wl_gate.sh [--prep] [--time] [--report] NEW_SHA [BASE_SHA]
#
# Run from the workstation (git bash) inside the repository. BASE_SHA defaults to NEW_SHA^1.
# No step flags = all three. Environment:
#   HOSTS    timing hosts (default "nozomi nico"; kotori is the second no-AVX2 box, the alternative to nozomi, e.g. HOSTS="kotori nico"; kanon, shiki also known; natsumi = loaned Zen 3 (Ryzen 5 5500), keke may need WL_RELAY=1; nico =
#            bare metal Ivy Bridge, the second opinion); the builds and links happen on keke.
#   S        the coordinator's scratchpad: if S/tools/vm_run.sh exists the long remote steps run
#            through it (queue, detach, VM_RUN_TIMING for the exclusive timing slot).
#   RETRY=0  do not re-measure a failing workload once before the verdict.
#   WL_RELAY=1  always relay --time copies through this machine (default: only when keke cannot log in to the host).
#   WL_COPY_ONLY=1  --time copies kits, inputs and scripts to the hosts and stops (no timing).
# Steps: --prep: git archive of both commits to keke:~/wl/src, wl_prep.sh (cargo xtask so, relink
# kits, inputs); --time: kits and inputs to each host (rsync from keke, or a tar relay through
# this machine when keke has no login there), wl_run.sh under the timing slot,
# glibc / BASE / NEW interleaved per workload; --report: wl_report.py per host (the table, the
# noise bands, WL-GATE PASS or FAIL; a failing workload is re-measured once first).
# Results: <host>:~/wl/out/<new>-<host>.tsv (+ -retry.tsv); the table is printed.
# Caches: keke:~/wl/{src,products,kit}/<sha> are reused.
set -u
steps=
while [ $# -gt 0 ]; do case $1 in --prep|--time|--report) steps="$steps $1"; shift ;; *) break ;; esac; done
[ -n "$steps" ] || steps="--prep --time --report"
new=$(git rev-parse "$1") || exit 2
base=$(git rev-parse "${2:-$1^1}") || exit 2
hosts=${HOSTS:-"nozomi nico"}
here=$(cd "$(dirname "$0")" && pwd)
ns=${new:0:9}
bs=${base:0:9}
keke="ssh -o BatchMode=yes claude@10.0.100.53"
hip() { # host -> address: the ONE host map (add new timing hosts here only)
    case $1 in
        keke) echo 10.0.100.53 ;;
        shiki) echo 10.0.100.66 ;;
        nico) echo 10.0.100.112 ;;
        kotori) echo 10.0.100.115 ;;
        nozomi) echo 10.0.100.118 ;;
        kanon) echo 10.0.100.81 ;;
        natsumi) echo 10.0.0.36 ;; # loaned Ryzen 5 5500 (Zen 3), other subnet, until the 10-25 spin-down
        *) echo "wl_gate: unknown host $1" >&2; exit 2 ;;
    esac
}
hssh() { # host
    case $1 in
        keke) echo "$keke" ;;
        *) if [ -r "$HOME/.ssh/id_claude" ]; then echo "ssh -o BatchMode=yes -i $HOME/.ssh/id_claude -o IdentitiesOnly=yes claude@$(hip "$1")"
           else echo "ssh -o BatchMode=yes claude@$(hip "$1")"; fi ;; # no id_claude on this machine (the Linux dispatcher): default keys
    esac
}
long() { # host label timing(0/1) command: detached under the queue when the scratchpad tools exist
    local h=$1 l=$2 t=$3 c=$4
    if [ -n "${S:-}" ] && [ -f "$S/tools/vm_run.sh" ]; then
        VM_RUN_TIMING=$t VM_NOWAIT=1 bash "$S/tools/vm_run.sh" "$h" "$l" "$c" | tail -1
    else
        # the command travels base64-encoded: a regex with ( ) or quotes in it (the ONLY= retry list) broke the nested quoting
        local c64; c64=$(printf '%s; echo $? > ~/logs/%s.done' "$c" "$l" | base64 -w0)
        $(hssh "$h") "mkdir -p ~/logs; (trap '' HUP; setsid nohup bash -lc 'echo $c64 | base64 -d | bash' > ~/logs/$l.log 2>&1 &) </dev/null" 
    fi
}
await() { # host label
    if [ -n "${S:-}" ] && [ -f "$S/tools/vm_wait.sh" ]; then
        while VM_WAIT_MAX_MIN=4 bash "$S/tools/vm_wait.sh" "$1" "$2" tail > /tmp/wl_wait.$$ 2>&1; [ $? = 3 ]; do :; done; tail -3 /tmp/wl_wait.$$; rm -f /tmp/wl_wait.$$
    else
        until $(hssh "$1") "test -f ~/logs/$2.done"; do sleep 30; done
    fi
}

wlsig() { # paths -> remote command printing "<path> <md5 of its find listing: name size mtime type; directories by name only, their sizes differ per filesystem>" per existing path
    echo "cd ~/wl 2>/dev/null || exit 0; for p in $*; do test -e \$p && echo \$p \$(find \$p \( -type d -printf '%P d\n' \) -o -printf '%P %s %Ts %y\n' | LC_ALL=C sort | md5sum | cut -c1-32); done"
}
relay() { # host: stream what the host lacks (or holds in another version) keke -> this machine -> host, one tar
    local h=$1 want todo
    $keke "cd ~/wl && test -d kit/glibc && test -d kit/$new && test -d kit/$base" || { echo "wl_gate: kits missing on keke"; return 1; }
    want="work kit/glibc kit/$new kit/$base $($keke 'cd ~/wl && echo bin/*')"
    todo=$(LC_ALL=C comm -23 <($keke "$(wlsig $want)" | LC_ALL=C sort) <($(hssh "$h") "$(wlsig $want)" | LC_ALL=C sort) | cut -d' ' -f1 | paste -sd' ')
    [ -n "$todo" ] || { echo "wl_gate: $h is up to date (relay)"; return 0; }
    echo "wl_gate: relaying to $h through this machine: $todo"
    # unpack into a temporary dir first, then move each path into place: an aborted stream leaves no half kit behind
    ( set -o pipefail
      $keke "cd ~/wl && tar -c $todo" | $(hssh "$h") "t=~/wl/.relay.\$\$; rm -rf \$t; mkdir -p \$t ~/wl/out ~/wl/kit ~/wl/bin && tar -x -C \$t && for p in $todo; do rm -rf ~/wl/\$p.old; { ! test -e ~/wl/\$p || mv -T ~/wl/\$p ~/wl/\$p.old; } && mv -T \$t/\$p ~/wl/\$p && rm -rf ~/wl/\$p.old || exit 1; done; rm -rf \$t" )
}
copy() { # host: kits, inputs and scripts to a timing host; keke -> host directly when keke can log in there, else relay
    local h=$1 ip
    ip=$(hip "$h") || return 1
    if [ "${WL_RELAY:-0}" != 1 ] && $keke "ssh -o BatchMode=yes -o ConnectTimeout=5 claude@$ip true" 2>/dev/null; then
        $keke "ssh -o BatchMode=yes claude@$ip 'mkdir -p ~/wl/out ~/wl/kit' && rsync -a -e 'ssh -o BatchMode=yes' ~/wl/bin ~/wl/work claude@$ip:wl/ && rsync -a -e 'ssh -o BatchMode=yes' ~/wl/kit/glibc ~/wl/kit/$new ~/wl/kit/$base claude@$ip:wl/kit/" || { echo "wl_gate: rsync to $h failed"; return 1; }
    else
        relay "$h" || { echo "wl_gate: relay to $h failed"; return 1; }
    fi
}

$keke "mkdir -p ~/wl/bin ~/wl/out"
. "$here/wl_install.sh"; wl_install_scripts || exit 1   # B = ~/wl/bin/v-<hash>, never overwritten; needed by EVERY step
case " $steps " in *" --prep "*)
    for s in $new $base; do
        $keke "test -f ~/wl/products/$s/libc.a || test -d ~/wl/src/$s" || \
            { git archive --format=tar "$s" | $keke "t=~/wl/src/.$s.\$\$; rm -rf \$t; mkdir -p \$t && tar -x -C \$t && { mv -T \$t ~/wl/src/$s 2>/dev/null || rm -rf \$t; }"; }
    done
    long keke "wl-prep-$ns" 0 "bash $B/wl_prep.sh $new $base"
    await keke "wl-prep-$ns"
    $keke "test \"\$(cat ~/logs/wl-prep-$ns.done)\" = 0" || { echo "wl_gate: prep failed (keke:~/logs/wl-prep-$ns.log)"; exit 1; } ;;
esac

case " $steps " in *" --time "*)
    for h in $hosts; do
        copy "$h" || exit 1
        [ "${WL_COPY_ONLY:-0}" = 1 ] && continue
        long "$h" "wl-$ns" 1 "bash $B/wl_run.sh ~/wl/out/$ns-$h.tsv glibc $base $new"
    done
    [ "${WL_COPY_ONLY:-0}" = 1 ] && exit 0
    for h in $hosts; do await "$h" "wl-$ns"; done ;;
esac

rc=0
case " $steps " in *" --report "*)
    for h in $hosts; do
        echo "== $h: $ns vs baseline $bs"
        # report-only runs: the scripts live on keke; ship them to the host through this machine (keke has no key for every host)
        $(hssh "$h") "test -d $B" || { $keke "tar -C ~/wl -c bin" | $(hssh "$h") "mkdir -p ~/wl && tar -C ~/wl -x"; } || { echo "wl_gate: cannot install scripts on $h"; exit 1; }
        out=$($(hssh "$h") "python3 $B/wl_report.py $new $base ~/wl/out/$ns-$h.tsv --host $h")
        r=$?
        if [ "${RETRY:-1}" = 1 ] && echo "$out" | grep -q '^WL-GATE FAIL'; then
            names=$(echo "$out" | sed -n 's/^WL-GATE FAIL: //p' | tr ',' '\n' | sed 's/ //g; s/(.*//' | paste -sd'|')
            echo "re-measuring: $names"
            long "$h" "wl-$ns-retry" 1 "ONLY='^($names)\$' bash $B/wl_run.sh ~/wl/out/$ns-$h-retry.tsv glibc $base $new"
            await "$h" "wl-$ns-retry"
            out=$($(hssh "$h") "python3 $B/wl_report.py $new $base ~/wl/out/$ns-$h.tsv ~/wl/out/$ns-$h-retry.tsv --host $h")
        fi
        echo "$out"
        echo "$out" | grep -q '^WL-GATE PASS' || rc=1
        # Attribution: top Liella symbols (instructions) for the workloads that failed a counter.
        names=$(echo "$out" | sed -n 's/^WL-GATE FAIL: //p' | grep -oE '[a-zA-Z0-9-]+\((instr|pf|sys_[a-z]+)\)' | sed 's/(.*//' | sort -u | paste -sd' ')
        if [ -n "$names" ]; then
            long "$h" "wl-$ns-attr" 1 "bash $B/wl_attr.sh $base $new $names > ~/wl/out/$ns-$h-attr.txt 2>&1"
            await "$h" "wl-$ns-attr"
            $(hssh "$h") "cat ~/wl/out/$ns-$h-attr.txt"
        fi
    done ;;
esac
exit $rc
