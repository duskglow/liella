#!/bin/sh
# Run netdb_diff.c (spec §16; phase 10): write the fixture /etc in a scratch directory, then run
# the lookups with that directory bind-mounted over /etc, in private mount and network namespaces
# where only the loopback interface is up, so that no lookup reads the system's configuration or
# reaches a network. Needs root: `sudo -n` (passwordless on the test machine). The dynamic loader's
# cache is copied into the fixture so that a dynamically linked program still starts.
#
# Usage: netdb_run.sh PROGRAM SCRATCH
# LIELLA_NSCACHE (the name-service cache switch, D-16.10), when set, reaches the root phases too
# (`nscache_mode.sh` runs the scripts with the cache on and off).
set -eu
prog=$1
dir=$2
rm -rf "$dir"
mkdir -p "$dir"
"$prog" "$dir"
if [ -f /etc/ld.so.cache ]; then
    cp /etc/ld.so.cache "$dir/etc/ld.so.cache"
fi
sudo -n env ${LIELLA_NSCACHE+"LIELLA_NSCACHE=$LIELLA_NSCACHE"} unshare -m -n sh -c 'ip link set lo up && mount --bind "$1/etc" /etc && exec "$2" "$1" lookup' sh "$dir" "$prog"
