#!/bin/sh
# ldcache_run.sh OUT - build a private root of tiny shared objects, let the reference system's
# ldconfig write ld.so.cache files for it (formats new, compat, old), and keep them in OUT.
# Format inspection only (spec Appendix D.3): nothing outside OUT is written; /etc is never touched.
set -eu
OUT=${1:?usage: ldcache_run.sh OUT}
LDCONFIG=${LDCONFIG:-/sbin/ldconfig.real}
rm -rf "$OUT/root"
mkdir -p "$OUT/root/etc" "$OUT/src"
R=$OUT/root
cd "$OUT/src"
printf 'int f(void){return 1;}\n' > f.c
lib() { # dir file soname [extra cc flags]
	mkdir -p "$R/$1"
	if [ -n "$3" ]; then so="-Wl,-soname,$3"; else so=""; fi
	gcc -shared -fPIC -o "$R/$1/$2" f.c $so ${4:-}
	# The soname link, as an installed library has it (ldconfig -X does not make it).
	if [ -n "$3" ] && [ "$3" != "$2" ]; then ln -sf "$2" "$R/$1/$3"; fi
}
lib usr/lib/x86_64-linux-gnu libalpha.so.1.2.3 libalpha.so.1
lib usr/lib/x86_64-linux-gnu libbeta.so.2.0 libbeta.so.2
lib usr/lib/x86_64-linux-gnu libnosoname.so ""
lib usr/local/lib libalpha.so.1.9 libalpha.so.1
lib usr/local/lib libgamma.so.10 libgamma.so.10
lib usr/local/lib libgamma.so.9 libgamma.so.9
lib usr/local/lib libZed.so.1 libZed.so.1
lib usr/local/lib/glibc-hwcaps/x86-64-v3 libgamma.so.10 libgamma.so.10
lib usr/local/lib/glibc-hwcaps/x86-64-v2 libgamma.so.10 libgamma.so.10
lib opt/x libdelta.so.1.0 libdelta.so.1
lib usr/local/lib libdelta.so.1.5 libdelta.so.1
# Sort-order probes: digit runs, case, length, punctuation.
for n in libv.so.1.10 libv.so.1.9 libv.so.1.09 libv.so.2 libv.so.a libV.so.1 lib10.so lib9.so libab.so libab.so.1 lib-x.so lib_x.so; do
	lib opt/sort "$n" "$n"
done
if gcc -m32 -shared -fPIC -o /dev/null f.c 2>/dev/null; then
	lib usr/lib32 libalpha.so.1 libalpha.so.1 -m32
fi
if gcc -mx32 -shared -fPIC -o /dev/null f.c 2>/dev/null; then
	lib usr/libx32 libalpha.so.1 libalpha.so.1 -mx32
fi
printf '/opt/x\n/usr/local/lib\n/opt/sort\n/usr/lib32\n/usr/libx32\n' > "$R/etc/ld.so.conf"
for fmt in new compat old; do
	"$LDCONFIG" -r "$R" -X -i -c "$fmt" -f /etc/ld.so.conf -C "/etc/ld.so.cache.$fmt" \
		> "$OUT/ldconfig-$fmt.log" 2>&1 || echo "ldconfig -c $fmt rc=$?" >> "$OUT/ldconfig-$fmt.log"
	cp "$R/etc/ld.so.cache.$fmt" "$OUT/cache.$fmt" 2>/dev/null || true
	"$LDCONFIG" -r "$R" -p -C "/etc/ld.so.cache.$fmt" > "$OUT/print-$fmt.txt" 2>&1 || true
done
# Second run with an empty configuration (only the trusted directories).
: > "$R/etc/ld.so.conf.empty"
"$LDCONFIG" -r "$R" -X -i -f /etc/ld.so.conf.empty -C /etc/ld.so.cache.trusted > "$OUT/ldconfig-trusted.log" 2>&1 || true
cp "$R/etc/ld.so.cache.trusted" "$OUT/cache.trusted" 2>/dev/null || true
ls -l "$OUT"
