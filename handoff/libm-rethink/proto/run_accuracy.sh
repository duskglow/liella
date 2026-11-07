#!/bin/bash
# LIBM-RETHINK prototype accuracy job (run under jobrun on the Zen 1 build host): build accuracy.c statically against
# Liella's products of revision $1 (cached by wl_percall in ~/wl) and run it with $2 operands per distribution.
set -u
v=${1:?revision}
n=${2:-100000000}
P=$HOME/wl/products/$v
CC=$HOME/wl/src/$v/scripts/liella-cc
D=$HOME/work/libm-rethink/proto
[ -f "$P/libc.a" ] && [ -x "$CC" ] || { echo "no cached products for $v"; exit 2; }
LIELLA_PRODUCTS=$P "$CC" -O2 -o "$D/accuracy" "$D/accuracy.c" -lm || exit 1
"$D/accuracy" "$n"
