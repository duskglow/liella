#!/bin/sh
# Run aio_diff.c (spec §11.11, POSIX asynchronous I/O) in a fresh scratch directory, which is
# removed afterwards. The probe prints no path, descriptor, PID or time; the scratch directory
# appears only as `<dir>` in the last line. Every section of the probe has its own alarm; the
# whole run has a timeout.
#
# Usage: aio_run.sh PROGRAM SCRATCH [extra]  (the program alone also takes DIR lio-eintr: only the
#   LIO_WAIT interrupted cases, for repeat loops)
#   extra: the observations outside the record (aio.md, "Outside the record").
set -eu
prog=$1
dir=$2
mode=${3-}
rm -rf "$dir"
mkdir -p "$dir"
dir=$(cd "$dir" && pwd -P)
trap 'rm -rf "$dir"' EXIT
if [ -n "$mode" ]; then
. "$(dirname "$0")/tscale.sh"
    timeout $((300 * TSCALE)) "$prog" "$dir" "$mode"
else
    timeout $((600 * TSCALE)) "$prog" "$dir"
fi
