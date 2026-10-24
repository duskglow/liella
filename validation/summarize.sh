#!/bin/bash
# LAB-ONLY: this script drives the project's own test hosts and is not needed to build or test Liella.
# Summarise validation results on keke (validation/README.md): per build, the counts of the
# package's own tests and of its gnulib tests, the failures, the hung tests, the check time and
# the load, and the gnulib replacements configure chose for one C library and not the other.
#
#   validation/summarize.sh LIBC_A LIBC_B PACKAGE...
#
# Re-derives the results of tar, make and gawk from their check logs (results_other.py), so a
# parser change applies without re-running a suite.
set -u
V=${VALIDATION:-$HOME/validation}
repo=$(cd "$(dirname "$0")/.." && pwd)
a=$1 b=$2
shift 2
cd "$V/results" || exit 1
for pkg in "$@"; do
    for libc in "$a" "$b"; do
        r=$libc-$pkg
        [ -f "$r.summary" ] || { echo "== $r: no run"; continue; }
        grep -E "^(PASS|FAIL|SKIP|XFAIL|XPASS|ERROR): " "$r.check.log" > "$r.results" 2>/dev/null
        case $pkg in
            tar | make | gawk) python3 "$repo/validation/results_other.py" "$pkg" "$r.check.log" >> "$r.results" ;;
        esac
        echo "== $r"
        grep -E "^(built-from|configure rc|build rc|check rc|check-start|check-end)" "$r.summary"
        # gnulib's tests run in gnulib-tests/ (or tests/ for some packages: named test-*).
        own=$(grep -vE "^[A-Z]+: (gnulib-tests/|test-)" "$r.results" | cut -d: -f1 | sort | uniq -c | tr '\n' ' ')
        gl=$(grep -E "^[A-Z]+: (gnulib-tests/|test-)" "$r.results" | cut -d: -f1 | sort | uniq -c | tr '\n' ' ')
        echo "own: $own"
        echo "gnulib: $gl"
        grep -E "^(FAIL|ERROR|XPASS): " "$r.results" | sed 's/^/  /'
        [ -s "$r.hung" ] && sed 's/^/  hung: /' "$r.hung"
        grep -c "undefined reference" "$r.build.log" | sed 's/^/  undefined references: /'
        grep "undefined reference" "$r.build.log" | sed "s/.*undefined reference to .\(.*\)'/\1/" | sort | uniq -c | sort -rn | head -8 | sed 's/^/   /'
    done
    if [ -f "$a-$pkg.replace" ] && [ -f "$b-$pkg.replace" ]; then
        echo "== replacements $pkg ($a < > $b)"
        diff "$a-$pkg.replace" "$b-$pkg.replace" | grep '^[<>]' | sed 's/^/  /'
        diff "$a-$pkg.cache" "$b-$pkg.cache" | grep '^[<>]' | sed 's/^/  /' | head -40
    fi
done
