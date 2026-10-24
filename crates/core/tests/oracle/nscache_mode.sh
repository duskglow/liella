#!/bin/sh
# Run a differential test's run script with Liella's name-service cache on or off (D-16.10), as
# the calling user, in a private mount namespace whose /dev/shm is a fresh tmpfs: the products put
# the user's shared region there, so a test never uses or leaves a region in the system's
# /dev/shm. `off` sets LIELLA_NSCACHE=0, which the run scripts pass on to their root phases; `on`
# leaves the environment as it was. Needs `sudo -n` (the namespace) and `setpriv`.
#
# Usage: nscache_mode.sh on|off COMMAND ARGS...
set -eu
mode=$1
shift
case $mode in
on) set -- env -u LIELLA_NSCACHE "$@" ;;
off) set -- env LIELLA_NSCACHE=0 "$@" ;;
*) echo "nscache_mode.sh: on or off, not $mode" >&2; exit 2 ;;
esac
exec sudo -n unshare -m env -i PATH="$PATH" sh -c 'u=$1; g=$2; shift 2; mount -t tmpfs -o mode=1777 tmpfs /dev/shm && exec setpriv --reuid="$u" --regid="$g" --init-groups "$@"' sh "$(id -u)" "$(id -g)" "$@"
