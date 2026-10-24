#!/bin/bash
# LAB-ONLY: this script drives the project's own test hosts and is not needed to build or test Liella.
# Re-run one configure test of a validation build against both C libraries (triage).
#
#   validation/conftest.sh CONFIG_LOG 'checking whether memchr works' [OUTDIR]
#
# Takes the program configure logged for that check ("failed program was:", in config.log,
# which the clean-room rules allow reading), writes it to OUTDIR/conftest.c, builds it with
# gcc (the host glibc) and with scripts/liella-cc (static Liella), runs both, and prints their
# exit statuses and output. Only config.log is read.
set -u
log=$1
check=$2
out=${3:-$(mktemp -d "${VALIDATION:-$HOME/validation}/conftest.XXXXXX")}
repo=$(cd "$(dirname "$0")/.." && pwd)
mkdir -p "$out"
awk -v c="$check" '
    index($0, c) && !found { found = 1; inside = 0; next }
    found && /^configure: failed program was:/ { inside = 1; next }
    found && inside && /^\| / { print substr($0, 3); next }
    found && inside && /^\|$/ { print ""; next }
    found && inside { exit }
    found && /^configure:[0-9]+: checking / { exit }
' "$log" > "$out/conftest.c"
if [ ! -s "$out/conftest.c" ]; then
    echo "no failed program logged for: $check" >&2
    exit 1
fi
echo "program: $out/conftest.c ($(wc -l < "$out/conftest.c") lines)"
for libc in glibc liella; do
    cc=gcc
    [ $libc = liella ] && cc=$repo/scripts/liella-cc
    if ! "$cc" -g -O2 -o "$out/conftest-$libc" "$out/conftest.c" > "$out/build-$libc.log" 2>&1; then
        echo "$libc: does not build:"; sed 's/^/    /' "$out/build-$libc.log" | head -20
        continue
    fi
    (cd "$out" && timeout 20 "./conftest-$libc" > "run-$libc.log" 2>&1)
    echo "$libc: exit $?"; sed 's/^/    /' "$out/run-$libc.log" | head -20
done
