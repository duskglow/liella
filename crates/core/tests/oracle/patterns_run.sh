#!/bin/sh
# Run patterns_diff.c (spec §14.7-14.10; phase 12) with an empty environment, a fresh fixture
# directory under SCRATCH, and the locales it uses (C.UTF-8 and en_US.UTF-8, compiled with
# localedef into SCRATCH/locales, which LOCPATH names). Each run has a timeout; the program
# runs every section in a child with an alarm and a parent-death signal.
#
# If COMPAT_PROGRAM is given (the same source built with -DPATTERNS_COMPAT_GLOB against the host
# C library, dynamically), its "compat" section is appended: glob@GLIBC_2.2.5 against the
# default glob. Liella's static products cannot select a symbol version, so the products
# comparison uses the first part only.
#
# LIELLA_NSCACHE (the name-service cache switch, D-16.10), when set, reaches the program too
# (`nscache_mode.sh` runs the script with the cache on and off; ~user expansion looks up passwd).
#
# Usage: patterns_run.sh PROGRAM SCRATCH [COMPAT_PROGRAM]
set -eu
prog=$1
dir=$2
compat=${3:-}
case $dir in /*) ;; *) dir=$(pwd)/$dir ;; esac
[ -d "$dir" ] && chmod -R u+rwx "$dir"
rm -rf "$dir"
mkdir -p "$dir/locales" "$dir/fx"
# localedef exits with 1 for warnings; the locale is usable then.
localedef -i C -f UTF-8 "$dir/locales/C.UTF-8" >/dev/null 2>&1 || [ $? -eq 1 ]
localedef -i en_US -f UTF-8 "$dir/locales/en_US.UTF-8" >/dev/null 2>&1 || [ $? -eq 1 ]
. "$(dirname "$0")/tscale.sh"
env -i PATH=/usr/bin:/bin LOCPATH="$dir/locales" ${LIELLA_NSCACHE+"LIELLA_NSCACHE=$LIELLA_NSCACHE"} timeout -s KILL $((1200 * TSCALE)) "$prog" "$dir/fx"
if [ -n "$compat" ]; then
    mkdir -p "$dir/fxc"
    env -i PATH=/usr/bin:/bin LOCPATH="$dir/locales" timeout -s KILL $((120 * TSCALE)) "$compat" "$dir/fxc" compat
fi
chmod -R u+rwx "$dir"
rm -rf "$dir"
