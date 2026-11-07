#!/bin/bash
# MACHINE-JCC-PAD: per-row counters of the staged bench programs, before (unpadded) and after (padded), on a
# Skylake-family host. Mode 1: instructions, cycles, uops_issued; mode mite: IDQ.MITE_UOPS in the uops column.
# Prints "ctr <variant> <exe> <mode> <row> <instr> <cycles> <bmiss> <l1d> <uinstr> <uops|mite>" lines.
set -u
W=~/work/machine-jcc-pad
CPU=${CPU:-3}
for v in before after; do
  if [ ! -d $W/$v ]; then mkdir -p $W/$v && tar -xf $W/$v.tar -C $W/$v || exit 1; fi
done
ls $W/after/target/liella/bench/ | head -40
for v in before after; do
  for exe in $W/$v/target/liella/bench/cbench_memref* $W/$v/target/liella/bench/cbench_hot*; do
    [ -f "$exe" ] && [ -x "$exe" ] || continue
    case "$exe" in *.c|*.o|*.tsv|*.d) continue ;; esac
    for mode in 1 mite; do
      CBENCH_COUNTERS=$mode taskset -c $CPU "$exe" 2>&1 >/dev/null | grep '^ctr' | sed "s|^ctr|ctr $v $(basename $exe) $mode|"
    done
  done
done
uptime
