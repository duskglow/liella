#!/bin/sh
# timing.sh <tarball> <tag> [tunables]: unpack the staged bench whole into a new directory and time
# cbench_hot, Liella vs glibc interleaved (--compare). Output: ~/work/TIMING-COV-STRNCMP-DIFF8/timing-<tag>.txt
set -e
W=~/work/TIMING-COV-STRNCMP-DIFF8
D=$W/stage-$2
rm -rf "$D"
mkdir -p "$D"
tar -xf "$1" -C "$D"
cd "$D"
X=$(find . -maxdepth 2 -name xtask-bin | head -1)
cd "$(dirname "$X")"
if [ -n "$3" ]; then
  ./xtask-bin bench --no-build --compare --runs 5 --tunables "$3" cbench_hot > "$W/timing-$2.txt" 2>&1
else
  ./xtask-bin bench --no-build --compare --runs 5 cbench_hot > "$W/timing-$2.txt" 2>&1
fi
