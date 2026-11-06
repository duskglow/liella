#!/bin/bash
# TIMING-COV-REGEX-FIRSTBYTE-IVB-2: cbench_patterns, base (origin/next) vs head, interleaved, each run a
# --compare against the host glibc with counters (cycles/instr per call), on the Ivy Bridge timing host.
# usage: run.sh <rounds-per-run> <alternations>
set -u
W=~/work/TIMING-COV-REGEX-FIRSTBYTE-IVB-2
RUNS=${1:-5}
ALT=${2:-3}
for side in base head; do
  if [ ! -x "$W/$side/xtask-bin" ]; then
    mkdir -p "$W/$side" && tar -xf "$W/$side.tar" -C "$W/$side" || { echo "extract $side failed"; exit 2; }
  fi
done
rc_all=0
for i in $(seq 1 "$ALT"); do
  for side in base head; do
    echo "=== run $i $side $(date -u +%H:%M:%SZ) load: $(cut -d' ' -f1-3 /proc/loadavg) MHz: $(grep -m4 'cpu MHz' /proc/cpuinfo | awk '{printf "%s ", $4}')"
    cd "$W/$side" || exit 2
    ./xtask-bin bench --compare --no-build --counters --runs "$RUNS" cbench_patterns
    rc=$?
    echo "=== run $i $side rc=$rc"
    [ $rc -ne 0 ] && rc_all=$rc
    mkdir -p "$W/out/$side-$i"
    cp bench-out/*.tsv "$W/out/$side-$i/" 2>/dev/null
  done
done
echo "rc_all=$rc_all"
exit $rc_all
