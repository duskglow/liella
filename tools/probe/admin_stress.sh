#!/bin/bash
# LAB-ONLY: this script drives the project's own test hosts and is not needed to build or test Liella.
# admin_stress.sh <bin> <warm-runs> <cold-batches> <jobs> <cpu-burners> <dd-burners> <deadline-secs>
# Usage: ORACLES=<role>/target/test-oracles tools/probe/admin_stress.sh <admin_products test binary> 3000 150 12 12 3 1500
# Runs under jobrun on an otherwise idle host; see tools/probe/README.md (admin reference). Namespace modes need unprivileged userns (not on sumire).
# All scratch under $STRESS_BASE (RAM). Bounded by deadline-secs. It does NOT watch for other jobs: check ~/bin/slots-status first.
bin=$1; warm=${2:-600}; cold=${3:-60}; J=${4:-8}; load=${5:-32}; ddn=${6:-4}; dl=${7:-1500}
base=${STRESS_BASE:-/mnt/zram/claude/ari2}   # must be RAM (zram/tmpfs)
mkdir -p $base; out=$base/out; rm -rf $out; mkdir -p $out
orc=${ORACLES:?set ORACLES to the role target/test-oracles}
end=$(( $(date +%s) + dl ))
pids=()
for i in $(seq $load); do ( while :; do :; done ) & pids+=($!); done
for i in $(seq $ddn); do ( while :; do dd if=/dev/zero of=$base/burn$i bs=1M count=512 2>/dev/null; sync -f $base/burn$i 2>/dev/null; done ) & pids+=($!); done
cleanup() { kill "${pids[@]}" 2>/dev/null; rm -f $base/burn*; }
trap cleanup EXIT
one() { # $1 = tag
  [ "$(date +%s)" -ge "$end" ] && { echo "DEADLINE $1"; return; }
  s=$(date +%s.%N)
  timeout 200 $bin admin_equals_the_reference --exact --nocapture >$out/$1.log 2>&1
  rc=$?
  e=$(date +%s.%N)
  echo "T $1 $(echo "$e - $s" | bc)" >> $out/times.txt
  grep -q "skipped" $out/$1.log && echo "skipped $1"
  if [ $rc -ne 0 ]; then echo "FAIL $1 rc=$rc"; mv $out/$1.log $out/FAIL-$1.log; else rm -f $out/$1.log; fi
}
export -f one; export bin out end
$bin admin_equals_the_reference --exact >/dev/null 2>&1   # build oracles once
echo "start $(date) load=$load dd=$ddn J=$J nproc=$(nproc)"
seq $warm | xargs -P $J -I{} bash -c 'one w{}' > $out/phaseA.txt 2>&1
echo "phaseA runs=$warm jobs=$J done=$(grep -c '^T ' $out/times.txt) fails=$(grep -c ^FAIL $out/phaseA.txt) skipped=$(grep -c skipped $out/phaseA.txt) deadline=$(grep -c ^DEADLINE $out/phaseA.txt)"
for b in $(seq $cold); do
  [ "$(date +%s)" -ge "$end" ] && { echo "phaseB stopped at batch $b (deadline)"; break; }
  rm -f $orc/* 2>/dev/null   # plain files only; subdirectories of other tests' oracles stay (their "Is a directory" noise is dropped)
  seq $J | xargs -P $J -I{} bash -c "one c${b}_{}" >> $out/phaseB.txt 2>&1
done
echo "phaseB batches<=$cold x $J fails=$(grep -c ^FAIL $out/phaseB.txt) skipped=$(grep -c skipped $out/phaseB.txt)"
echo "total timed runs=$(grep -c '^T ' $out/times.txt) max=$(awk '/^T /{if($3>m)m=$3}END{print m}' $out/times.txt)s"
ls $out | grep FAIL | head
echo "end $(date)"
