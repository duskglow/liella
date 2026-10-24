#!/bin/bash
# tools/prof/ann.sh <row> <symbol-substring> [n] -- the n (default 14) hottest instructions of one function in a
# profile made by prof.sh (Liella's own code). Usage: OUT=~/prof/rows tools/prof/ann.sh strcmp_equal_16 strcmp 20
set -u
O=${OUT:-$HOME/prof/rows}; cd "$O" || exit 2
sym=$(grep -F "$2" "$1.top" | head -1 | awk '{print $NF}')
[ -z "$sym" ] && { echo "no symbol $2"; exit 1; }
echo "## $1 :: ${sym:0:90}"
tmp=$O/.ann.$$; perf annotate -i "$1.data" --stdio -s "$sym" 2>/dev/null > "$tmp"
grep -E '^ +[0-9]+\.[0-9]+ +:' "$tmp" | sort -t: -k1 -gr | head -"${3:-14}" | sed -E 's/ +/ /g' | cut -c1-120
echo "instructions: $(grep -cE '^ +[0-9.]+ +:' "$tmp")"
rm -f "$tmp"
