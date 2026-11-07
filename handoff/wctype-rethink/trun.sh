#!/bin/sh
# WCTYPE-RETHINK timing run: alternates the Liella (static, with prototypes) and host-library probes,
# <pairs> process pairs of 3 internal rounds each. usage: trun.sh <dir> <pairs>
set -u
D=$1
P=$2
cd "$D" || exit 2
i=0
while [ "$i" -lt "$P" ]; do
  ./wc-liella time 3 "t-liella-$i.tsv" || exit 3
  ./wc-glibc time 3 "t-glibc-$i.tsv" || exit 4
  i=$((i + 1))
done
grep -m1 "model name" /proc/cpuinfo
uptime
echo done
