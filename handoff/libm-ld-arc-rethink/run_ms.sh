#!/bin/bash
# LIBM-LD-ARC-RETHINK-2: which symbols and instructions carry the microcode-sequencer uops and the idle cycles of
# Liella's asinl against atan2l (Ivy Bridge, user mode, pinned). usage: run_ms.sh <dir holding bin/>
B=$1/bin; T=$(mktemp -d); P="taskset -c 1"
echo "cpu:$(grep -m1 'model name' /proc/cpuinfo | cut -d: -f2) load: $(cut -d' ' -f1-3 /proc/loadavg)"
for c in asinl atan2l_as; do
    for e in idq.ms_uops cycles cycle_activity.cycles_no_execute; do
        perf record -q -e "$e:u" -c 2003 -o "$T/p.data" $P "$B/arcprobe-liella" "$c" 400000 thr >/dev/null 2>&1
        echo "== $c $e"
        perf report -i "$T/p.data" --stdio --sort symbol 2>/dev/null | grep -v '^#' | grep -v '^$' | head -8 | cut -c1-150
    done
done
for e in idq.ms_uops cycle_activity.cycles_no_execute; do
    perf record -q -e "$e:u" -c 2003 -o "$T/p.data" $P "$B/arcprobe-liella" asinl 400000 thr >/dev/null 2>&1
    for s in $(perf report -i "$T/p.data" --stdio --no-demangle --sort symbol 2>/dev/null | grep -v '^#' | grep -v '^$' | head -2 | awk '{print $NF}'); do
        echo "== annotate $e $s (lines >= 2%)"
        perf annotate -i "$T/p.data" --stdio --no-demangle -s "$s" 2>/dev/null | awk '$1+0 >= 2' | head -16 | cut -c1-150
    done
done
echo "load at end: $(cut -d' ' -f1-3 /proc/loadavg)"
rm -rf "$T"
