#!/bin/sh
# Run users_diff.c's two phases (spec §17.3-17.6; phase 10): the stream and utmp cases in a
# scratch directory, then the database lookups with that directory's fixture /etc bind-mounted
# over /etc in a private mount namespace, so that nothing on the system is read or written. The
# namespace needs root: `sudo -n` (passwordless on the test machine). The dynamic loader's cache is
# copied into the fixture so that a dynamically linked program still starts.
#
# Usage: users_run.sh PROGRAM SCRATCH
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
sudo -n env ${LIELLA_NSCACHE+"LIELLA_NSCACHE=$LIELLA_NSCACHE"} unshare -m sh -c 'mount --bind "$1/etc" /etc && exec "$2" "$1" nss' sh "$dir" "$prog"
