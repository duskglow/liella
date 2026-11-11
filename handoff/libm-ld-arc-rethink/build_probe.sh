#!/bin/bash
# LIBM-LD-ARC-RETHINK: build arcprobe against glibc, Liella, and Liella with the phase markers of instrument.py
# (applied in the role, restored with fresh mtimes, products rebuilt clean), then a fenced smoke run.
# usage: build_probe.sh <role dir> <out dir>
set -eux
R=$1; O=$2; H=$R/handoff/libm-ld-arc-rethink
. ~/.cargo/env
mkdir -p "$O/bin"
cd "$R"
cargo xtask so
gcc -O2 -o "$O/bin/arcprobe-glibc" "$H/arcprobe.c" -lm
LIELLA_PRODUCTS=$R/target/liella sh scripts/liella-cc -O2 -o "$O/bin/arcprobe-liella" "$H/arcprobe.c" -lm
python3 "$H/instrument.py" apply "$R" "$O/backup"
trap 'python3 "$H/instrument.py" restore "$R" "$O/backup"' EXIT
cargo xtask so
LIELLA_PRODUCTS=$R/target/liella sh scripts/liella-cc -O2 -o "$O/bin/arcprobe-inst" "$H/arcprobe.c" -lm
python3 "$H/instrument.py" restore "$R" "$O/backup"
trap - EXIT
cargo xtask so
cp "$H/run_probe.sh" "$O/"
for b in glibc liella inst; do "$O/bin/arcprobe-$b" asinl 20000 tsc; "$O/bin/arcprobe-$b" atan2l_as 20000 tsc; done
