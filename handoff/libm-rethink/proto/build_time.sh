#!/bin/bash
# LIBM-RETHINK-2: build the prototype timing binaries on the build host (the timing hosts have no compiler). Runs under
# jobrun from the role: Liella's products (cargo xtask so, incremental), then t_liella (static against Liella) and
# t_glibc (the system glibc, dynamic with -z now as validation/wl_percall.sh links it), then a short smoke run of both.
# Usage: build_time.sh <role dir> <work dir holding proto_exp.c and proto_time.c>
set -eu
R=${1:?role dir}
D=${2:?work dir}
cd "$R"
cargo xtask so
cd "$D"
LIELLA_PRODUCTS=$R/target/liella "$R/scripts/liella-cc" -O2 -o t_liella proto_time.c -lm
gcc -O2 -Wl,-z,now -o t_glibc proto_time.c -lm
file t_liella t_glibc
./t_liella 100000 3 smoke-liella
./t_glibc 100000 3 smoke-glibc
sha256sum t_liella t_glibc
