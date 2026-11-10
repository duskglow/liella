#!/bin/bash
# STARTUP-DATA-WINDOWS (D-X.50): rewrite crates/libc-so/startup-data.keys from the products of <tree>.
# usage: tools/hottext/startup_data.sh <tree with built products> [--replace]
#   (STARTUP_DATA_PRODUCTS=<dir>: read the products from <dir>, a saved copy, instead of <tree>/target/liella)
#   Links crates/core/tests/oracle/page_faults.c as crates/core/tests/hot_text_products.rs links it and
#   records the data objects `page_faults true` touches (tools/hottext/startup_data.py), for the profile
#   in <tree>/target/liella/.profile; the union over profiles is kept. regen.sh runs it after the
#   start-up text trace (it needs startup-text.keys of the same profile). Needs perf (page-faults events).
set -eu
here=$(cd "$(dirname "$0")" && pwd)
T=$(cd "$1" && pwd); shift
P=${STARTUP_DATA_PRODUCTS:-$T/target/liella}
profile=$(cat "$P/.profile" 2>/dev/null || true)
case $profile in fast|release) ;; *) echo "startup_data: $P/.profile: want fast or release, got '$profile'" >&2; exit 1;; esac
work=$(mktemp -d "${TMPDIR:-/tmp}/startupdata.XXXXXX")
trap 'rm -rf "$work"' EXIT
cc -std=gnu17 -O1 -static -nostdlib -fno-builtin "$P/liella-crt1.o" "$T/crates/core/tests/oracle/page_faults.c" \
    "$P/libc.a" -lgcc -o "$work/page_faults"
python3 "$here/startup_data.py" "$work/page_faults" "$here/../../crates/libc-so/startup-text.keys" "$profile" \
    "$here/../../crates/libc-so/startup-data.keys" "$@"
