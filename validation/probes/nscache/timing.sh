#!/bin/sh
# Interleaved timing of the name-service cache per lookup (no spec clause: measurement only, D-16.10).
# usage: timing.sh <rounds> <user> <percall-A> <percall-B>
# Each round runs A and B (order alternates by round), each once with LIELLA_NSCACHE=1 and once with
# LIELLA_NSCACHE=0 (same environment size), 4 lookups per process. Prints the median ns per binary, mode and
# call, then the break-even lookup count: a process making n lookups gains from the cache when
#   n > 2 + (c1 + c2 - u1 - u2) / (u - h)
# with c1, c2 the cached 1st/2nd call, u1, u2 the uncached 1st/2nd, u the uncached later call (call 3-4) and
# h the cached hit (call 3-4). Run percall-* once with LIELLA_NSCACHE=1 first so the region exists.
set -u
R=${1:?rounds}; U=${2:?user}; A=${3:?binary A}; B=${4:?binary B}
OUT=$(mktemp)
trap 'rm -f "$OUT"' EXIT
env LIELLA_NSCACHE=1 "$A" 4 "$U" >/dev/null
env LIELLA_NSCACHE=1 "$B" 4 "$U" >/dev/null
i=0
while [ "$i" -lt "$R" ]; do
  if [ $((i % 2)) -eq 0 ]; then order="A B"; else order="B A"; fi
  for t in $order; do
    if [ "$t" = A ]; then bin=$A; else bin=$B; fi
    for m in 1 0; do
      env LIELLA_NSCACHE=$m "$bin" 4 "$U" | awk -v t="$t" -v m="$m" '{ print t, m, $2, $NF }' >> "$OUT"
    done
  done
  i=$((i + 1))
done
echo "binaries: A=$A B=$B rounds=$R load=$(cut -d' ' -f1-3 /proc/loadavg)"
for t in A B; do
  for m in 1 0; do
    for c in 1 2 3 4; do
      v=$(awk -v t="$t" -v m="$m" -v c="$c" '$1 == t && $2 == m && $3 == c { print $4 }' "$OUT" | sort -n |
        awk '{ a[NR] = $1 } END { if (NR == 0) print 0; else if (NR % 2) print a[(NR + 1) / 2]; else print (a[NR / 2] + a[NR / 2 + 1]) / 2 }')
      echo "med $t $m $c $v"
    done
  done
done | tee "$OUT.med"
for t in A B; do
  awk -v t="$t" '$2 == t { v[$3 "." $4] = $5 }
    END {
      c1 = v["1.1"]; c2 = v["1.2"]; h = (v["1.3"] + v["1.4"]) / 2
      u1 = v["0.1"]; u2 = v["0.2"]; u = (v["0.3"] + v["0.4"]) / 2
      printf "%s: cached 1st %.1f us 2nd %.1f us hit %.2f us | uncached 1st %.1f us 2nd %.1f us later %.2f us\n", t, c1 / 1000, c2 / 1000, h / 1000, u1 / 1000, u2 / 1000, u / 1000
      if (u > h) printf "%s: break-even n > %.2f lookups (extra attach cost %.1f us, saving %.2f us per later lookup)\n", t, 2 + (c1 + c2 - u1 - u2) / (u - h), (c1 + c2 - u1 - u2) / 1000, (u - h) / 1000
      else printf "%s: no break-even (hit not cheaper)\n", t
    }' "$OUT.med"
done
rm -f "$OUT.med"
