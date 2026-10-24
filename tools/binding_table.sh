#!/bin/sh
# Print "symbol<TAB>version<TAB>bind" (GLOBAL/WEAK) for every defined, versioned dynamic symbol of a
# shared object (default: the system glibc libc.so.6), sorted. Used to build spec/abi/binding.tsv and to
# compare Liella's libc.so.6 with it:  tools/binding_table.sh [libc.so.6]
so=${1:-/lib/x86_64-linux-gnu/libc.so.6}
readelf --dyn-syms -W "$so" | awk '
  $7 != "UND" && $7 != "ABS" && ($5 == "GLOBAL" || $5 == "WEAK") && NF >= 8 {
    n = $8; v = "";
    i = index(n, "@");
    if (i > 0) { v = substr(n, i + 1); n = substr(n, 1, i - 1); sub(/^@/, "", v) }
    printf "%s\t%s\t%s\n", n, v, $5
  }' | LC_ALL=C sort
