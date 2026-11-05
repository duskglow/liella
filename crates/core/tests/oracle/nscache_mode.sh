#!/bin/sh
# Run a differential test's run script with Liella's name-service cache on or off (D-16.10), as
# the calling user, in a private mount namespace whose /dev/shm is a fresh tmpfs: the products put
# the user's shared region there, so a test never uses or leaves a region in the system's
# /dev/shm. `off` sets LIELLA_NSCACHE=0, which the run scripts pass on to their root phases; `on`
# leaves the environment as it was. Needs `sudo -n` (the namespace) and `setpriv`.
#
# A tree that itself lives under /dev/shm (a `cargo xtask plants` copy in RAM) would vanish under
# the fresh tmpfs, and the run script with it ("cannot open .../catalogs_run.sh"). So each
# top-level directory of /dev/shm holding the working directory or a path argument (`/dev/shm/x/..`
# or `NAME=/dev/shm/x/..`) is bound back into the new /dev/shm, from a descriptor opened on the old
# one before the mount. The shared region is a top-level file: it still starts fresh.
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
keep=
for p in "$PWD" "$@"; do
	case $p in *=/dev/shm/?*) p=${p#*=} ;; esac
	case $p in
	/dev/shm/?*)
		n=${p#/dev/shm/}
		n=${n%%/*}
		case " $keep " in *" $n "*) ;; *) keep="$keep $n" ;; esac
		;;
	esac
done
exec sudo -n unshare -m env -i PATH="$PATH" sh -c 'u=$1; g=$2; k=$3; shift 3; exec 3</dev/shm; mount -t tmpfs -o mode=1777 tmpfs /dev/shm || exit 1; for n in $k; do mkdir "/dev/shm/$n" && mount --no-canonicalize --bind "/proc/$$/fd/3/$n" "/dev/shm/$n" || exit 1; done; exec 3<&-; exec setpriv --reuid="$u" --regid="$g" --init-groups "$@"' sh "$(id -u)" "$(id -g)" "$keep" "$@"
