#!/bin/sh
# Run syslog_diff.c (spec §26.1; phase 13) where it cannot reach the system log: as root, in a
# private mount namespace (`unshare -m --propagation private`), a fresh tmpfs is mounted over /dev,
# so /dev/log, /dev/console and everything else under /dev belong to the probe (only /dev/null is
# recreated, as a device node, and the marker file `.liella-syslog-fixture`, without which the
# program refuses to run). The program then runs as the calling user (`setpriv`), with an empty
# environment, binds its own /dev/log and reads what the library sends. The tmpfs and every file
# in it vanish with the namespace. The namespace needs root: `sudo -n` (passwordless on the test
# machine).
#
# Usage: syslog_run.sh PROGRAM
# LIELLA_NSCACHE (the name-service cache switch, D-16.10), when set, reaches the program too
# (`nscache_mode.sh` runs the scripts with the cache on and off).
set -eu
prog=$(cd "$(dirname "$1")" && pwd -P)/$(basename "$1")
uid=$(id -u)
gid=$(id -g)
. "$(dirname "$0")/tscale.sh"
sudo -n env LC_ALL=C ${LIELLA_NSCACHE+"LIELLA_NSCACHE=$LIELLA_NSCACHE"} timeout $((120 * TSCALE)) unshare -m --propagation private sh -c '
set -e
mount -t tmpfs -o size=16m,mode=0755,uid="$2",gid="$3" liella-syslog-dev /dev
mknod -m 666 /dev/null c 1 3
touch /dev/.liella-syslog-fixture
exec setpriv --reuid="$2" --regid="$3" --clear-groups env -i PATH=/usr/bin:/bin ${LIELLA_NSCACHE+"LIELLA_NSCACHE=$LIELLA_NSCACHE"} "$1"' sh "$prog" "$uid" "$gid"
