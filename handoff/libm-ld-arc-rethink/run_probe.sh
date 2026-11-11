#!/bin/bash
# LIBM-LD-ARC-RETHINK: arcprobe matrix on this host (no compiler needed). Lines: "R bin case mode n ev=count..."
# (perf stat, user mode, pinned to CPU 1; the summary takes (count(n) - count(0 calls)) / n), "tsc ..." (fenced
# per-call and per-phase TSC ticks), "M mb_* ..." (latency chains). usage: run_probe.sh <dir holding bin/>
B=$1/bin; N=400000; T=$(mktemp -d); P="taskset -c 1"
pmu=$(cat /sys/bus/event_source/devices/cpu/caps/pmu_name 2>/dev/null)
echo "cpu:$(grep -m1 'model name' /proc/cpuinfo | cut -d: -f2) pmu=$pmu load: $(cut -d' ' -f1-3 /proc/loadavg)"
case $pmu in
ivybridge) EV="uops_issued.any idq.ms_uops ld_blocks.store_forward fp_assist.any other_assists.any_wb_assist
  machine_clears.memory_ordering arith.fpu_div_active cycle_activity.cycles_no_execute resource_stalls.any
  int_misc.recovery_cycles idq.mite_uops idq.dsb_uops ild_stall.lcp idq_uops_not_delivered.core fp_comp_ops_exe.x87
  uops_dispatched_port.port_0 uops_dispatched_port.port_1 uops_dispatched_port.port_5 other_assists.avx_to_sse
  other_assists.sse_to_avx" ;;
skylake*) EV="uops_issued.any idq.ms_uops ld_blocks.store_forward fp_assist.any other_assists.any machine_clears.count
  arith.divider_active cycle_activity.stalls_total resource_stalls.any int_misc.recovery_cycles idq.mite_uops
  idq.dsb_uops ild_stall.lcp idq_uops_not_delivered.core uops_executed.x87 uops_dispatched_port.port_0
  uops_dispatched_port.port_1 uops_dispatched_port.port_5" ;;
*) EV="ld_blocks.store_forward machine_clears.any uops_retired.ms assists.any topdown_be_bound.all
  topdown_bad_speculation.all topdown_fe_bound.all topdown_retiring.all" ;;
esac
ok="branch-misses:u"
for e in $EV; do perf stat -e "$e:u" true >/dev/null 2>&1 && ok="$ok $e:u" || echo "dropped $e"; done
set -- $ok; G=()
while [ $# -gt 0 ]; do g=$1; shift; for k in 1 2 3; do [ $# -gt 0 ] && { g="$g,$1"; shift; }; done; G+=("$g"); done
cnt() { # bin case n events [mode]
    perf stat -x, -o "$T/p" -e "cycles:u,instructions:u,$4" $P "$B/arcprobe-$1" "$2" "$3" "${5:-thr}" >"$T/o" 2>&1
    awk -F, '$1 ~ /^[0-9.]+$/ { printf "%s=%s ", $3, $1 }' "$T/p"
}
CASES="asinl acosl atan2l_as atan2l_q asinl_fix acosl_fix atan2l_fix asinl_hi asinl_lo acosl_hi"
for g in "${G[@]}"; do
    for b in glibc liella; do
        echo "R $b ident thr 0 $(cnt $b ident 0 "$g")"
        for c in $CASES; do echo "R $b $c thr $N $(cnt $b $c $N "$g") $(cat "$T/o")"; done
    done
done
for b in glibc liella; do
    echo "R $b ident lat 0 $(cnt $b ident 0 "${G[0]}" lat)"
    for c in asinl acosl atan2l_as asinl_fix atan2l_fix; do echo "R $b $c lat $N $(cnt $b $c $N "${G[0]}" lat) $(cat "$T/o")"; done
done
for b in glibc liella inst; do
    for c in ident asinl acosl atan2l_as atan2l_q asinl_fix acosl_fix atan2l_fix asinl_hi asinl_lo; do echo "$b $($P "$B/arcprobe-$b" $c 40000 tsc)"; done
done
for m in mb_imul mb_fsqrt mb_fpatan mb_fyl2x mb_fdiv mb_divsd mb_sqrtsd mb_ldmxcsr mb_fldcw mb_mul64 mb_mulhi128 mb_cvt mb_f80rt; do
    echo "M $m 2000000 $(cnt glibc $m 2000000 "${G[0]}") $(cat "$T/o")"
done
echo "load at end: $(cut -d' ' -f1-3 /proc/loadavg)"
rm -rf "$T"
