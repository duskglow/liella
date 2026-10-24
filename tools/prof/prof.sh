#!/bin/bash
# tools/prof/prof.sh <tree> [bench] -- flat `perf` profiles of LIELLA'S OWN bench binary, one row at a time.
# Usage: ROWS="row1 row2" tools/prof/prof.sh ~/w/bench-<task> [cbench_opt]
#   <tree>  a built tree (`cargo xtask bench` or `cargo xtask so` has put the C bench exe in target/liella/bench);
#   ROWS    CBENCH_ONLY row prefixes (default: every row, one profile each);  OUT  output dir (default ~/prof/rows).
# Writes $OUT/<row>.data and $OUT/<row>.top (the 25 hottest symbols); prints the wall time of each run.
# perf only works without sudo (paranoid=1) on a process of yours: profile Liella's binaries, never the system's.
set -u
tree=${1:?usage: prof.sh <tree> [bench]}; bench=${2:-cbench_opt}
B=$tree/target/liella/bench; O=${OUT:-$HOME/prof/rows}; mkdir -p "$O"
[ -x "$B/$bench" ] || { echo "prof: no $B/$bench (build it: cargo xtask bench --only $bench)"; exit 2; }
cg="--call-graph lbr"; perf record -q $cg -o /dev/null true 2>/dev/null || cg=""   # LBR call graphs where the CPU has them
echo "callgraph: ${cg:-none}"
rows=(${ROWS:-}); [ ${#rows[@]} -eq 0 ] && rows=("")
for r in "${rows[@]}"; do
  l=${r:-all}
  t0=$(date +%s.%N)
  CBENCH_ONLY=$r perf record -q $cg -F 1999 -o "$O/$l.data" -- "$B/$bench" > /dev/null 2>&1
  t1=$(date +%s.%N)
  perf report -i "$O/$l.data" --stdio --no-children 2>/dev/null | grep -E '^ +[0-9]' | head -25 > "$O/$l.top"
  printf '%s %.1fs\n' "$l" "$(echo "$t1 - $t0" | bc)"
done
