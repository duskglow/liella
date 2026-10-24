#!/bin/sh
# Run mntent_diff.c's two phases (spec §13.13; phase 12):
#  1. the unprivileged cases on fixture files in SCRATCH (the program removes them) and the
#     structure of the real /proc/self/mounts; the program then leaves SCRATCH/etc (a fixture
#     /etc: fstab, mtab, a marker file) and the mount points SCRATCH/m/<names>.
#  2. `root`: as root in a private mount namespace (`unshare -m --propagation private`), with a
#     small tmpfs mounted on each SCRATCH/m/<name> (names with a space, a tab, a newline, a
#     backslash and a `#`; one read-only, one with a backslash in its source, one with extra
#     options) and SCRATCH/etc bind-mounted over /etc, so the fstab functions and _PATH_MOUNTED
#     read only the fixture and every mount vanishes with the namespace. The program refuses to
#     run that phase unless /etc holds the fixture's marker file. The namespace needs root:
#     `sudo -n` (passwordless on the test machine). The dynamic loader's cache is copied into the
#     fixture so that a dynamically linked program still starts.
#
# Usage: mntent_run.sh PROGRAM SCRATCH
# LIELLA_NSCACHE (the name-service cache switch, D-16.10), when set, reaches the root phase too
# (`nscache_mode.sh` runs the scripts with the cache on and off).
set -eu
# The mounts are made in glob order, which the record shows: fix the collation.
LC_ALL=C
export LC_ALL
prog=$1
dir=$2
rm -rf "$dir"
mkdir -p "$dir"
dir=$(cd "$dir" && pwd -P)
trap 'rm -rf "$dir"' EXIT
. "$(dirname "$0")/tscale.sh"
timeout $((120 * TSCALE)) "$prog" "$dir"
if [ -f /etc/ld.so.cache ]; then
    cp /etc/ld.so.cache "$dir/etc/ld.so.cache"
fi
sudo -n env LC_ALL=C ${LIELLA_NSCACHE+"LIELLA_NSCACHE=$LIELLA_NSCACHE"} timeout $((120 * TSCALE)) unshare -m --propagation private sh -c '
set -e
for d in "$1"/m/*/; do
    case "$d" in
        */ro/) mount -t tmpfs -o ro,size=64k "read only" "$d" ;;
        */bs/) mount -t tmpfs -o size=64k "back\\slash" "$d" ;;
        */opts/) mount -t tmpfs -o size=64k,nosuid,nodev,noexec,mode=0700,uid=1,gid=2 "with opts" "$d" ;;
        *) mount -t tmpfs -o size=64k "src with space" "$d" ;;
    esac
done
mount --bind "$1/etc" /etc
exec "$2" "$1" root' sh "$dir" "$prog"
