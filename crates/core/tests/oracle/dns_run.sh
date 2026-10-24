#!/bin/sh
# Run dns_diff.c (spec §16.1-16.4; phase 10): write every scenario's fixture /etc and run the
# offline message functions, then run each scenario with its fixture bind-mounted over /etc, in
# private mount and network namespaces where only the loopback interface is up; the program starts
# its own DNS server on 127.0.0.1:53 there. Needs root: `sudo -n` (passwordless on the test
# machine). The dynamic loader's cache is copied into each fixture so that a dynamically linked
# program still starts.
#
# Usage: dns_run.sh PROGRAM SCRATCH
# LIELLA_NSCACHE (the name-service cache switch, D-16.10), when set, reaches the root phases too
# (`nscache_mode.sh` runs the scripts with the cache on and off).
set -eu
prog=$1
dir=$2
rm -rf "$dir"
mkdir -p "$dir"
"$prog" "$dir"
for s in basic search trustad noaaaa usevc notfound_return notfound_continue unavail; do
    if [ -f /etc/ld.so.cache ]; then
        cp /etc/ld.so.cache "$dir/$s/etc/ld.so.cache"
    fi
    sudo -n env ${LIELLA_NSCACHE+"LIELLA_NSCACHE=$LIELLA_NSCACHE"} unshare -m -n sh -c 'ip link set lo up && mount --bind "$1/$3/etc" /etc && exec "$2" "$1" "$3"' sh "$dir" "$prog" "$s"
done
