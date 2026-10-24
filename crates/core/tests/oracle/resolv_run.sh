#!/bin/sh
# Run resolv_diff.c (spec §16.2, §16.4; phase 10, part 4): write every scenario's fixture /etc,
# then run each scenario with its fixture bind-mounted over /etc, in private mount and network
# namespaces where only the loopback interface is up; the program starts its own DNS servers on
# loopback addresses there, so nothing reaches a network. Needs root: `sudo -n` (passwordless on
# the test machine). The dynamic loader's cache is copied into each fixture so that a dynamically
# linked program still starts; the scenarios run with an empty environment and the "C" locale.
#
# Usage: resolv_run.sh PROGRAM SCRATCH [SCENARIO...]
# LIELLA_NSCACHE (the name-service cache switch, D-16.10), when set, reaches the root phases too
# (`nscache_mode.sh` runs the scripts with the cache on and off).
set -eu
prog=$1
dir=$2
shift 2
rm -rf "$dir"
mkdir -p "$dir"
"$prog" "$dir"
if [ $# -eq 0 ]; then
    set -- conf conf2 conf3 noconf servers rotate norotate silent1 silent2 refused edns plain single reopen sortlist search notld env envspace envempty aliases reload noreload modify idn gaia
fi
for s in "$@"; do
    if [ -f /etc/ld.so.cache ]; then
        cp /etc/ld.so.cache "$dir/$s/etc/ld.so.cache"
    fi
. "$(dirname "$0")/tscale.sh"
    sudo -n unshare -m -n env -i PATH=/usr/sbin:/usr/bin:/sbin:/bin ${LIELLA_NSCACHE+"LIELLA_NSCACHE=$LIELLA_NSCACHE"} sh -c 'ip link set lo up && mount --bind "$1/$3/etc" /etc && exec timeout '"$((60 * TSCALE))"' "$2" "$1" "$3"' sh "$dir" "$prog" "$s"
done
