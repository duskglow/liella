#!/bin/sh
# Run process_diff.c's three phases (spec §18.3-18.9; phase 11):
#  1. the unprivileged cases, with the fixture written into SCRATCH;
#  2. `root`: POSIX_SPAWN_RESETIDS and a real-time policy, as root in a private mount namespace
#     (`unshare -m`); the program is copied to a tmpfs mounted over /mnt inside that namespace,
#     so that a child running with real UID 65534 can execute it. The copy is read through a
#     descriptor opened BEFORE the mount: the tree may itself live under /mnt (the NVMe roles,
#     /mnt/nvme/claude/w/nX), and the new mount hides every path below /mnt;
#  3. `noshell`: system, popen and the execvp shell fallback when /bin/sh cannot be executed, with
#     a non-executable file bind-mounted over /bin/sh inside a private mount namespace.
# Nothing on the system is written: the mounts exist only in the namespaces and vanish with them.
# The namespaces need root: `sudo -n` (passwordless on the test machine).
#
# Usage: process_run.sh PROGRAM SCRATCH
# LIELLA_NSCACHE (the name-service cache switch, D-16.10), when set, reaches the root phases too
# (`nscache_mode.sh` runs the scripts with the cache on and off).
set -eu
prog=$1
dir=$2
rm -rf "$dir"
mkdir -p "$dir"
"$prog" "$dir"
sudo -n env ${LIELLA_NSCACHE+"LIELLA_NSCACHE=$LIELLA_NSCACHE"} unshare -m sh -c 'exec 3<"$2" && mount -t tmpfs tmpfs /mnt && cat <&3 >/mnt/process_diff && exec 3<&- && chmod 755 /mnt/process_diff && cd / && exec /mnt/process_diff "$1" root' sh "$dir" "$prog"
sudo -n env ${LIELLA_NSCACHE+"LIELLA_NSCACHE=$LIELLA_NSCACHE"} unshare -m sh -c 'mount --bind "$1/notashell" /bin/sh && exec "$2" "$1" noshell' sh "$dir" "$prog"
