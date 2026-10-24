#!/bin/bash
# LAB-ONLY: this script drives the project's own test hosts and is not needed to build or test Liella.
# MEMCMP-SIZE-MIX: build the histogram-instrumented kit of SHA (from the role tree ROLE) and run every workload of
# validation/wl_workloads.sh once with MCS_OUT set; then sum the dumps with validation/mcs_hist.py.
#   mcs_run.sh ROLE_DIR SHA OUT_DIR
set -u
role=$1 sha=$2 out=$3
. ~/.cargo/env
mkdir -p "$out" ~/wl/src/"$sha"
rsync -a --exclude /target --exclude /.git --exclude /.synced "$role"/ ~/wl/src/"$sha"/ || exit 1
echo "copied $(date +%T)"
cp "$role"/validation/mcs_hist.py "$out"/ || exit 1
bash "$role"/validation/wl_prep.sh "$sha" > "$out"/prep.log 2>&1 || { echo "wl_prep failed"; tail -20 "$out"/prep.log; exit 1; }
. "$role"/validation/wl_workloads.sh
kit=$HOME/wl/kit/$sha/bin
for d in "${WL_DEFS[@]}"; do
    name=${d%%|*} cmd=${d#*|}
    c=${cmd//@/$kit}
    rm -f "$out/$name.bin"
    (cd ~/wl/work && MCS_OUT="$out/$name.bin" timeout 900 sh -c "$c" > /dev/null 2>&1)
    echo "$name rc=$? bytes=$(stat -c %s "$out/$name.bin" 2>/dev/null || echo 0)"
done
python3 "$out"/mcs_hist.py "$out"/*.bin > "$out"/summary.txt
echo done
