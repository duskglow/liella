#!/bin/bash
# TIMING-IVB-STRSCAN-DISPATCH-2: interleaved before/after timing of the 11 rows on a timing host.
# usage: run.sh <rounds-per-run> <alternations>
set -u
W=~/work/TIMING-IVB-STRSCAN-DISPATCH-2
RUNS=${1:-5}
ALT=${2:-3}
R=$(cat "$W/rows")
for side in before after; do
  if [ ! -x "$W/$side/xtask-bin" ]; then
    mkdir -p "$W/$side" && tar -xf "$W/$side.tar" -C "$W/$side" || { echo "extract $side failed"; exit 2; }
  fi
done
rc_all=0
for i in $(seq 1 "$ALT"); do
  for side in before after; do
    echo "=== run $i $side $(date -u +%H:%M:%SZ) load: $(cut -d' ' -f1-3 /proc/loadavg)"
    cd "$W/$side" || exit 2
    ./xtask-bin bench --no-build --staged --counters --runs "$RUNS" --rows "$R"
    rc=$?
    echo "=== run $i $side rc=$rc"
    [ $rc -ne 0 ] && rc_all=$rc
    mkdir -p "$W/out/$side-$i"
    cp target/liella/bench/*.tsv "$W/out/$side-$i/" 2>/dev/null
  done
done
echo "rc_all=$rc_all"
exit $rc_all
