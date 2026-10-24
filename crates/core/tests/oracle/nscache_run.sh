#!/bin/sh
# Run nscache_check.c (D-16.10, the integrated name-service cache) as root in private mount and
# network namespaces: a tmpfs on /dev/shm (the shared regions live there, so no region of the
# system's /dev/shm is ever used or left), a tmpfs on /mnt (the program's copies, one of them
# setuid root), the scratch directory bind-mounted over /etc (the program writes its fixture
# there), and only the loopback interface up (the program serves DNS on 127.0.0.1:53). Every mount
# vanishes with the namespace. No path of the tree is used after the tmpfs mounts hide /dev/shm and
# /mnt (the tree may live under either: the NVMe roles are under /mnt/nvme, a RAM copy under
# /dev/shm): the scratch directory is bound over /etc first, and the program is read through a
# descriptor opened before the mounts and copied into the /mnt tmpfs. Needs root: `sudo -n` (passwordless on the test machine).
#
# Usage: nscache_run.sh PROGRAM SCRATCH
set -eu
prog=$1
dir=$2
rm -rf "$dir"
mkdir -p "$dir/etc"
. "$(dirname "$0")/tscale.sh"
sudo -n unshare -m -n env -i PATH=/usr/sbin:/usr/bin:/sbin:/bin sh -c 'ip link set lo up && mount --bind "$1/etc" /etc && exec 3<"$2" && mount -t tmpfs -o mode=1777 tmpfs /dev/shm && mount -t tmpfs -o mode=755 tmpfs /mnt && cat <&3 >/mnt/nscache_main && exec 3<&- && chmod 755 /mnt/nscache_main && exec timeout '"$((300 * TSCALE))"' /mnt/nscache_main main' sh "$dir" "$prog"
