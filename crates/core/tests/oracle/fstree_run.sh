#!/bin/sh
# Run fstree_diff.c's two phases (spec §13.1-13.3, §13.8-13.9; phase 12):
#  1. the unprivileged cases, with the fixture written into SCRATCH and removed at the end. The
#     permission cases (unreadable and unsearchable directories, a read-only directory for the
#     temporary-name functions) need a non-root user: run the harness as an ordinary user.
#  2. `root`: chroot, a lazy unmount of the working directory, nested mounts for FTW_MOUNT and
#     FTS_XDEV, and device nodes, as root in a private mount namespace (`unshare -m`) with a tmpfs
#     mounted over SCRATCH, so nothing it creates reaches a disk and every mount vanishes with the
#     namespace. The namespace needs root: `sudo -n` (passwordless on the test machine).
# The first line of the output names the scratch directory's file system (`stat -f`): directory
# order, which several lines record, depends on it.
#
# Usage: fstree_run.sh PROGRAM SCRATCH
# LIELLA_NSCACHE (the name-service cache switch, D-16.10), when set, reaches the root phase too
# (`nscache_mode.sh` runs the scripts with the cache on and off).
set -eu
prog=$1
dir=$2
rm -rf "$dir"
mkdir -p "$dir"
dir=$(cd "$dir" && pwd -P)
trap 'chmod -R u+rwx "$dir" 2>/dev/null; rm -rf "$dir"' EXIT
echo "stat -f -c %T: $(stat -f -c %T "$dir")"
. "$(dirname "$0")/tscale.sh"
timeout $((600 * TSCALE)) "$prog" "$dir"
sudo -n env ${LIELLA_NSCACHE+"LIELLA_NSCACHE=$LIELLA_NSCACHE"} timeout $((300 * TSCALE)) unshare -m --propagation private sh -c 'mount -t tmpfs -o size=16m tmpfs "$1" && exec "$2" "$1" root' sh "$dir" "$prog"
