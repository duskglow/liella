#!/bin/sh
# run.sh DIR: build and run the executable-stack probe in DIR (sources es.c there).
set -u
cd "$1" || exit 2
printf 'int es_lib(void) { return 1; }\n' > lib.c
cc -O1 -o es es.c -ldl -lpthread || exit 3
cc -O1 -Wl,-z,execstack -o es_x es.c -ldl -lpthread || exit 3
cc -O1 -shared -fPIC -Wl,-z,execstack -o libx.so lib.c || exit 3
cc -O1 -shared -fPIC -Wl,-z,noexecstack -o libn.so lib.c || exit 3
for f in es es_x libx.so libn.so; do
  printf '%s: ' "$f"; readelf -lW "$f" | grep GNU_STACK | awk '{print $1, $NF, $(NF-1)}'
done
echo "== es (no execstack), dlopen libn.so"; timeout 20 ./es ./libn.so; echo "rc $?"
echo "== es (no execstack), dlopen libx.so"; timeout 20 ./es ./libx.so; echo "rc $?"
echo "== es_x (program asks execstack), no dlopen"; timeout 20 ./es_x; echo "rc $?"
ldd --version | head -1
