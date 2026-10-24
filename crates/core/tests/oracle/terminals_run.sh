#!/bin/sh
# Run terminals_diff.c's two phases (spec §19; phase 11): the unprivileged cases on ptys the
# program creates in a scratch directory, then the ttyent and ttyname cases as root in a private
# mount namespace (`unshare -m`, whose mounts are private, so nothing reaches the system) with
# the scratch directory's fixture /etc bind-mounted over /etc; inside it the program mounts a
# tmpfs or a new devpts instance over /dev/pts, binds that instance's ptmx over /dev/ptmx and a
# tmpfs over /proc, all of which vanish with the namespace. The namespace needs root: `sudo -n`
# (passwordless on the test machine). The dynamic loader's cache is copied into the fixture so
# that a dynamically linked program still starts.
#
# Usage: terminals_run.sh PROGRAM SCRATCH
# LIELLA_NSCACHE (the name-service cache switch, D-16.10), when set, reaches the root phases too
# (`nscache_mode.sh` runs the scripts with the cache on and off).
set -eu
prog=$1
dir=$2
rm -rf "$dir"
mkdir -p "$dir"
. "$(dirname "$0")/tscale.sh"
timeout $((120 * TSCALE)) "$prog" "$dir" </dev/null
if [ -f /etc/ld.so.cache ]; then
    cp /etc/ld.so.cache "$dir/etc/ld.so.cache"
fi
timeout $((120 * TSCALE)) sudo -n env ${LIELLA_NSCACHE+"LIELLA_NSCACHE=$LIELLA_NSCACHE"} unshare -m sh -c 'mount --bind "$1/etc" /etc && exec "$2" "$1" ns' sh "$dir" "$prog" </dev/null
