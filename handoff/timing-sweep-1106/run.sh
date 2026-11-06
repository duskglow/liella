#!/bin/bash
# TIMING-SWEEP-1106: timing-host run of the non-libm bench groups from the staged products (same groups as 1105).
# Output ~/work/TIMING-SWEEP-1106/out (bench.log ends with rc=N; counters.tsv, rounds.tsv, ...).
R=$HOME/work/TIMING-SWEEP-1106
O=$R/out
mkdir -p "$O" "$R/tree"
cd "$R/tree" || exit 3
if [ ! -x xtask-bin ]; then tar xf "$R/stage.tar" || exit 4; fi
ONLY=cbench_aio,cbench_fstree,cbench_hot,cbench_locale,cbench_malloc,cbench_memref,cbench_netdb,cbench_nscache,cbench_opt,cbench_patterns,cbench_process,cbench_services,cbench_startup,cbench_stdio,cbench_thread,cbench_time,cbench_unmeasured
./xtask-bin bench --no-build --staged --compare --counters --only $ONLY > "$O/bench.log" 2>&1
rc=$?
for d in bench-out target/liella/bench; do [ -d "$d" ] && cp "$d"/*.tsv "$O/" 2>/dev/null; done
echo "rc=$rc" >> "$O/bench.log"
exit $rc
