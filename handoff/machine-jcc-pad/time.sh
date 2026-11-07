#!/bin/bash
# MACHINE-JCC-PAD timing on a core without the JCC erratum: the staged before (unpadded) and after (padded) trees,
# each timed against the host C library with --compare, interleaved over two rounds (b a a b), cbench_memref and
# cbench_hot. Read the Liella/glibc ratio per row: after should not be above before beyond the run-to-run spread.
set -u
W=~/work/machine-jcc-pad
for v in before after; do
  if [ ! -d $W/$v ]; then mkdir -p $W/$v && tar -xf $W/$v.tar -C $W/$v || exit 1; fi
done
for v in before after after before; do
  for b in cbench_memref cbench_hot; do
    echo "=== round $v $b"
    (cd $W/$v && ./xtask-bin bench --no-build --compare $b)
  done
done
uptime
