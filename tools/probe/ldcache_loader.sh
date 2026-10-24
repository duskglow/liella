#!/bin/sh
# ldcache_loader.sh OUT - black-box checks of the reference loader against private ld.so.cache files
# (spec Appendix D.3). Needs ldcache_run.sh's tree in OUT and ldcache_dump.py next to this script.
# Each cache is bind-mounted over /etc/ld.so.cache inside an unprivileged user+mount namespace
# (unshare -r -m): the host's /etc is never changed. Prints one "CHECK name: result" line per check.
set -eu
OUT=${1:?usage: ldcache_loader.sh OUT}
HERE=$(cd "$(dirname "$0")" && pwd)
LDSO=/lib64/ld-linux-x86-64.so.2
LDCONFIG=${LDCONFIG:-/sbin/ldconfig.real}
R=$OUT/root
L=$OUT/loader
mkdir -p "$L"
cd "$L"
printf 'int main(void){return 0;}\n' > main.c
gcc -o main main.c -Wl,--no-as-needed \
	-L"$R/usr/local/lib" -l:libgamma.so.10 -l:libalpha.so.1.9 \
	-L"$R/opt/x" -l:libdelta.so.1.0 -L"$R/opt/sort" -l:libv.so.1.9
# A cache with absolute paths: ldconfig over the fixture directories (plus the trusted ones).
printf '%s\n' "$R/opt/x" "$R/usr/local/lib" "$R/opt/sort" "$R/usr/lib/x86_64-linux-gnu" > abs.conf
"$LDCONFIG" -X -i -f "$L/abs.conf" -C "$L/abs.cache" > abs.log 2>&1
list() { # cache -> the loader's resolution of main's four libraries
	env -u LD_LIBRARY_PATH -u LD_PRELOAD unshare -r -m sh -c \
		'mount --bind "$1" /etc/ld.so.cache && exec "$2" --list "$3"' sh "$1" "$LDSO" "$L/main" 2>&1 |
		grep -E 'lib(gamma|alpha|delta|v)\.' | sed "s|$R||g; s/ (0x[0-9a-f]*)//; s/^[[:space:]]*//" | tr '\n' ';'
	echo
}
echo "CHECK ldconfig-cache: $(list "$L/abs.cache")"
# Hand-written caches (ldcache_dump.py write), same paths, varied order / flags / hwcaps.
w() { # name, then spec lines on stdin
	cat > "$1.tsv"
	python3 -I "$HERE/ldcache_dump.py" write "$1.cache" "$1.tsv"
}
P=$R
w mine-same <<EOF
0303	libv.so.1.9	$P/opt/sort/libv.so.1.9
0303	libgamma.so.10	$P/usr/local/lib/glibc-hwcaps/x86-64-v2/libgamma.so.10	x86-64-v2
0303	libgamma.so.10	$P/usr/local/lib/glibc-hwcaps/x86-64-v3/libgamma.so.10	x86-64-v3
0303	libgamma.so.10	$P/usr/local/lib/libgamma.so.10
0303	libdelta.so.1	$P/opt/x/libdelta.so.1
0303	libdelta.so.1	$P/usr/local/lib/libdelta.so.1
0303	libalpha.so.1	$P/usr/local/lib/libalpha.so.1
0303	libalpha.so.1	$P/usr/lib/x86_64-linux-gnu/libalpha.so.1
EOF
echo "CHECK writer-same: $(list "$L/mine-same.cache")"
w mine-swapped <<EOF
0303	libv.so.1.9	$P/opt/sort/libv.so.1.9
0303	libgamma.so.10	$P/usr/local/lib/libgamma.so.10
0303	libgamma.so.10	$P/usr/local/lib/glibc-hwcaps/x86-64-v3/libgamma.so.10	x86-64-v3
0303	libgamma.so.10	$P/usr/local/lib/glibc-hwcaps/x86-64-v2/libgamma.so.10	x86-64-v2
0303	libdelta.so.1	$P/usr/local/lib/libdelta.so.1
0303	libdelta.so.1	$P/opt/x/libdelta.so.1
0303	libalpha.so.1	$P/usr/lib/x86_64-linux-gnu/libalpha.so.1
0303	libalpha.so.1	$P/usr/local/lib/libalpha.so.1
EOF
echo "CHECK writer-swapped: $(list "$L/mine-swapped.cache")"
w mine-hwcaps <<EOF
0303	libv.so.1.9	$P/opt/sort/libv.so.1.9
0303	libgamma.so.10	$P/usr/local/lib/glibc-hwcaps/x86-64-v3/libgamma.so.10	x86-64-v4
0303	libgamma.so.10	$P/usr/local/lib/glibc-hwcaps/x86-64-v2/libgamma.so.10	x86-64-v2
0303	libgamma.so.10	$P/usr/local/lib/libgamma.so.10
0003	libdelta.so.1	$P/opt/x/libdelta.so.1
0303	libdelta.so.1	$P/usr/local/lib/libdelta.so.1
0803	libalpha.so.1	$P/usr/local/lib/libalpha.so.1
0303	libalpha.so.1	$P/usr/lib/x86_64-linux-gnu/libalpha.so.1
EOF
echo "CHECK writer-hwcaps-flags: $(list "$L/mine-hwcaps.cache")"
# Which glibc-hwcaps entry the loader takes (keke: x86-64-v3 supported, x86-64-v4 not).
G=$P/usr/local/lib
hw() { # name, then the libgamma rows on stdin; the other three libraries as in mine-same
	{ printf '0303\tlibv.so.1.9\t%s/opt/sort/libv.so.1.9\n' "$P"; cat
	  printf '0303\tlibdelta.so.1\t%s/opt/x/libdelta.so.1\n' "$P"
	  printf '0303\tlibalpha.so.1\t%s/usr/local/lib/libalpha.so.1\n' "$P"; } | w "$1"
	echo "CHECK $1: $(list "$L/$1.cache" | tr ';' '\n' | grep gamma)"
}
printf '0303\tlibgamma.so.10\t%s\tx86-64-v2\n0303\tlibgamma.so.10\t%s\n' \
	"$G/glibc-hwcaps/x86-64-v2/libgamma.so.10" "$G/libgamma.so.10" | hw hw-v2-plain
printf '0303\tlibgamma.so.10\t%s\tx86-64-v3\n0303\tlibgamma.so.10\t%s\tx86-64-v2\n0303\tlibgamma.so.10\t%s\n' \
	"$G/glibc-hwcaps/x86-64-v3/libgamma.so.10" "$G/glibc-hwcaps/x86-64-v2/libgamma.so.10" "$G/libgamma.so.10" | hw hw-v3-v2-plain
printf '0303\tlibgamma.so.10\t%s\tx86-64-v4\n0303\tlibgamma.so.10\t%s\tx86-64-v3\n0303\tlibgamma.so.10\t%s\n' \
	"$G/glibc-hwcaps/x86-64-v2/libgamma.so.10" "$G/glibc-hwcaps/x86-64-v3/libgamma.so.10" "$G/libgamma.so.10" | hw hw-v4-v3-plain
printf '0303\tlibgamma.so.10\t%s\tx86-64-v2\n0303\tlibgamma.so.10\t%s\tx86-64-v4\n0303\tlibgamma.so.10\t%s\n' \
	"$G/glibc-hwcaps/x86-64-v2/libgamma.so.10" "$G/glibc-hwcaps/x86-64-v3/libgamma.so.10" "$G/libgamma.so.10" | hw hw-v2-v4-plain
printf '0303\tlibgamma.so.10\t%s\tno-such-level\n0303\tlibgamma.so.10\t%s\n' \
	"$G/glibc-hwcaps/x86-64-v2/libgamma.so.10" "$G/libgamma.so.10" | hw hw-unknown-plain
# What ldconfig -p calls each flag value (labels only).
: > flags.tsv
for f in 0000 0001 0002 0003 0100 0103 0203 0303 0403 0503 0603 0703 0803 0903 0a03 0b03 0c03 0d03 0f03; do
	printf '%s\tlibf%s.so\t/x/libf%s.so\n' "$f" "$f" "$f" >> flags.tsv
done
python3 -I "$HERE/ldcache_dump.py" write flags.cache flags.tsv
"$LDCONFIG" -p -C "$L/flags.cache" > flags.print 2>&1 || true
sed 's/^[[:space:]]*/CHECK flag-label: /' flags.print | grep 'libf'
"$LDCONFIG" -p -C "$L/mine-same.cache" > mine-same.print 2>&1 || true
echo "CHECK ldconfig-p-reads-writer: $(grep -c '=>' mine-same.print) entries"
grep 'hwcap' mine-same.print | sed 's/^[[:space:]]*/CHECK writer-hwcap-print: /'
