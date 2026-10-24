#!/bin/sh
# WEAK-EXPORTS: a static program that defines its own fgetc (WEAK in glibc's libc) links against
# libc.a and uses its own definition, and libc.so.6 exports fgetc WEAK.
# Usage: conformance/weak_fgetc_static.sh <dir with the products: target/liella>
set -e
d=${1:-target/liella}
t=$(mktemp -d)
trap 'rm -rf "$t"' EXIT
cat > "$t/p.c" <<'EOF'
#include <stdio.h>
int fgetc(FILE *f) { (void)f; return 'Q'; }
int main(void) { return fgetc(stdin) == 'Q' ? 0 : 1; }
EOF
cc -static -nostdlib -nostartfiles "$t/p.c" "$d/liella-crt1.o" "$d/libc.a" -lgcc -o "$t/p" 2>"$t/err" || { cat "$t/err"; exit 1; }
set +e
"$t/p" < /dev/null
rc=$?
[ "$rc" = 0 ] || { echo "own fgetc not used (exit $rc)"; exit 1; }
readelf --dyn-syms -W "$d/libc.so.6" | grep -q ' WEAK .* fgetc@@GLIBC' || { echo "fgetc not WEAK in libc.so.6"; exit 1; }
echo "ok"
