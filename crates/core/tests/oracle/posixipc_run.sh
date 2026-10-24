#!/bin/sh
# Run posixipc_diff.c (spec §25.3-25.4; phase 13) where it cannot touch the system's message queues
# or /dev/shm: as root, in a private IPC namespace (`unshare -i`: every queue lives and dies with
# it) and a private mount namespace (`unshare -m --propagation private`) in which /dev/mqueue is a
# fresh mqueue mount of the new IPC namespace and /dev/shm a fresh tmpfs holding the marker file
# `.liella-posixipc-fixture`, without which the program refuses to run. The shell checks that its
# IPC namespace differs from that of process 1 before it mounts anything. Phases:
#  1. `setup`, as root: root-owned queues and /dev/shm entries for the permission cases;
#  2. `user`, as the calling user (`setpriv`, empty environment): everything else;
#  3. `ext4`, as the calling user, with SCRATCH/shmdir (on the scratch directory's file system)
#     bind-mounted over /dev/shm;
#  4. `nodev`, as the calling user, with an empty tmpfs mounted over /dev (no /dev/shm) and the
#     marker `.liella-posixipc-nodev`.
# The mounts vanish with the namespaces; SCRATCH is removed at the end. The namespaces need root:
# `sudo -n` (passwordless on the test machine).
#
# Usage: posixipc_run.sh PROGRAM SCRATCH
# LIELLA_NSCACHE (the name-service cache switch, D-16.10), when set, reaches every phase too
# (`nscache_mode.sh` runs the scripts with the cache on and off).
set -eu
prog=$(cd "$(dirname "$1")" && pwd -P)/$(basename "$1")
dir=$2
rm -rf "$dir"
mkdir -p "$dir/shmdir"
dir=$(cd "$dir" && pwd -P)
trap 'rm -rf "$dir"' EXIT
uid=$(id -u)
gid=$(id -g)
. "$(dirname "$0")/tscale.sh"
sudo -n env LC_ALL=C ${LIELLA_NSCACHE+"LIELLA_NSCACHE=$LIELLA_NSCACHE"} timeout $((120 * TSCALE)) unshare -i -m --propagation private sh -c '
set -e
if [ "$(readlink /proc/self/ns/ipc)" = "$(readlink /proc/1/ns/ipc)" ]; then
    echo "not in a private IPC namespace" >&2
    exit 1
fi
mount -t mqueue liella-mqueue /dev/mqueue
mount -t tmpfs -o size=4m,mode=1777 liella-shm /dev/shm
touch /dev/shm/.liella-posixipc-fixture
"$1" setup
setpriv --reuid="$3" --regid="$4" --clear-groups env -i PATH=/usr/bin:/bin ${LIELLA_NSCACHE+"LIELLA_NSCACHE=$LIELLA_NSCACHE"} "$1" user
mount --bind "$2/shmdir" /dev/shm
touch /dev/shm/.liella-posixipc-fixture
setpriv --reuid="$3" --regid="$4" --clear-groups env -i PATH=/usr/bin:/bin ${LIELLA_NSCACHE+"LIELLA_NSCACHE=$LIELLA_NSCACHE"} "$1" ext4
mount -t tmpfs -o size=64k,mode=0755 liella-nodev /dev
touch /dev/.liella-posixipc-nodev
exec setpriv --reuid="$3" --regid="$4" --clear-groups env -i PATH=/usr/bin:/bin ${LIELLA_NSCACHE+"LIELLA_NSCACHE=$LIELLA_NSCACHE"} "$1" nodev' sh "$prog" "$dir" "$uid" "$gid"
