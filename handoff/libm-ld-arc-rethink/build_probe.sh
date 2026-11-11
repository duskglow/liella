#!/bin/bash
# LIBM-LD-ARC-RETHINK: build arcprobe against glibc, Liella, and Liella with the phase markers of instrument.py
# (applied in the role, restored with fresh mtimes, products rebuilt clean), then a fenced smoke run.
# If the fully instrumented product build fails, print where the start-up state statics went in the archives
# (diagnosis for the report) and build with the math-crate marks only (boundary crate untouched).
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
touch "$O/marker"
if ! cargo xtask so; then
    set +x
    echo "DIAG: fully instrumented product build failed; start-up state statics in archives built since the marker:"
    find "$R/target" -name '*.a' -newer "$O/marker" | while read -r a; do
        echo "DIAG archive $a"
        nm -A "$a" 2>/dev/null | grep -E 'SYSINFO_EHDR|AUXV_WORDS' | sed 's/^/DIAG nm /' | head -8
        readelf -SW "$a" 2>/dev/null | grep -E 'SYSINFO_EHDR|AUXV_WORDS' | sed 's/^/DIAG sec /' | head -8
    done
    set -x
    python3 "$H/instrument.py" restore "$R" "$O/backup"
    python3 "$H/instrument.py" apply "$R" "$O/backup" math-only
    echo "DIAG: retrying with math-crate marks only (phases 2, 3, 4)"
    cargo xtask so
fi
LIELLA_PRODUCTS=$R/target/liella sh scripts/liella-cc -O2 -o "$O/bin/arcprobe-inst" "$H/arcprobe.c" -lm
python3 "$H/instrument.py" restore "$R" "$O/backup"
trap - EXIT
cargo xtask so
cp "$H/run_probe.sh" "$O/"
for b in glibc liella inst; do "$O/bin/arcprobe-$b" asinl 20000 tsc; "$O/bin/arcprobe-$b" atan2l_as 20000 tsc; done
