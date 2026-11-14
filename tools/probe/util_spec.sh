#!/bin/sh
# Black-box probe of the reference system's libc utilities for spec §31 (UTIL-SPEC).
#
# Runs each command with a fixed empty environment (PATH, HOME and a scratch directory only) and
# records its stdout (`  | `), stderr (`  ! `) and exit status (`  rc=`). It runs the programs and
# reads only what they print and the files they write: no program's own text (several of them are
# scripts) is read or searched. Run as an ordinary user, never as root.
#
# Usage: sh tools/probe/util_spec.sh > spec/behaviour/utilities.glibc.txt
# Masks: the scratch directory prints as <dir>; addresses (0x and 8 to 16 hex digits) as 0xADDR;
# process ids as <pid>; tzselect's current-time lines as <now>. `rc=` is the status of the whole
# command line (its last command).
set -u
T=$(mktemp -d /tmp/util-spec.XXXXXX) || exit 2
trap 'rm -rf "$T"' EXIT
P=/usr/sbin:/usr/bin:/sbin:/bin

mask() {
	sed -e "s#$T#<dir>#g" -e 's/0x[0-9a-f]\{8,16\}/0xADDR/g' -e 's/process [0-9][0-9]*/process <pid>/g' \
		-e 's/\(is now:\).*/\1	<now>/'
}

# run LABEL 'shell command' [stdin file]
run() {
	printf '== %s\n' "$1"
	printf '  $ %s\n' "$2" | mask
	in=${3:-/dev/null}
	(cd "$T" && timeout 60 env -i PATH="$P" HOME="$T" T="$T" sh -c "$2" >"$T/.out" 2>"$T/.err" <"$in")
	rc=$?
	head -n 40 "$T/.out" | mask | sed 's/^/  | /'
	n=$(wc -l <"$T/.out"); [ "$n" -gt 40 ] && printf '  | ... (%s lines)\n' "$n"
	head -n 20 "$T/.err" | mask | sed 's/^/  ! /'
	printf '  rc=%s\n' "$rc"
}

printf '# util_spec.sh on %s; ' "$(. /etc/os-release; echo "$PRETTY_NAME")"
dpkg-query -W -f '${Package} ${Version}; ' libc-bin libc-dev-bin locales tzdata dpkg 2>/dev/null
printf '\n'

# --- paths and kinds (31.1)
run paths 'for c in ldconfig ldd getent getconf iconv locale localedef zic zdump tzselect gencat pldd iconvconfig; do p=$(command -v $c) || p=missing; printf "%s %s %s\n" $c $p "$(file -b -L $p 2>/dev/null | cut -d, -f1)"; done'
run ldso-link 'readlink /usr/bin/ld.so'
run ld.so.conf 'cat /etc/ld.so.conf; ls /etc/ld.so.conf.d'
run dpkg-needs-ldconfig 'mkdir -p r/var/lib/dpkg/updates r/var/lib/dpkg/info && : > r/var/lib/dpkg/status && env PATH=/usr/bin dpkg --root=$T/r --force-not-root --configure -a'

# --- ldconfig (31.2)
run ldconfig-help 'ldconfig --help'
run ldconfig-version 'ldconfig --version | head -n 1'
run ldconfig-p 'ldconfig -p | head -n 3; ldconfig -p | tail -n 1'
run ldconfig-p-missing 'ldconfig -p -C $T/nosuch.cache'
run ldconfig-user 'ldconfig'
run ldconfig-c-old-help 'ldconfig --help | grep -e "--format"'
: >"$T/empty.conf"
run ldconfig-c-old 'ldconfig -c old -C $T/old.cache -f $T/empty.conf -X; r=$?; head -c 11 old.cache; echo; exit $r'
run ldconfig-c-new 'ldconfig -C $T/new.cache -f $T/empty.conf -X; r=$?; head -c 20 new.cache; echo; exit $r'
mkdir -p "$T/rr/etc"
run ldconfig-r-user 'ldconfig -r $T/rr'
mkdir -p "$T/lib"
printf 'int f(void){return 1;}\n' >"$T/f.c"
for v in 1.2 1.9 1.10; do cc -shared -fPIC -o "$T/lib/libfoo.so.$v" -Wl,-soname,libfoo.so.1 "$T/f.c"; done
cc -shared -fPIC -o "$T/lib/libnos.so.1.0" "$T/f.c"
ln -s libfoo.so.1.10 "$T/lib/libfoo.so"
run ldconfig-n-links 'ldconfig -n -v $T/lib 2>&1 | sort; find lib -type l -printf "%p -> %l\n" | sort'

# --- ldd (31.3)
printf 'int main(void){return 0;}\n' >"$T/m.c"
cc -static -o "$T/static" "$T/m.c" 2>/dev/null
cc -static-pie -o "$T/staticpie" "$T/m.c" 2>/dev/null
run ldd-help 'ldd --help'
run ldd-version 'ldd --version | head -n 1'
run ldd-none 'ldd'
run ldd-missing 'ldd $T/nosuch'
run ldd-notelf 'ldd /etc/hostname'
run ldd-dynamic 'ldd /usr/bin/true'
run ldd-static 'file -b static | cut -d, -f1-2; ldd $T/static'
run ldd-static-pie 'file -b staticpie | cut -d, -f1-2; ldd $T/staticpie'
run ldd-two 'ldd /usr/bin/true /usr/bin/false'
run ldd-v 'ldd -v /usr/bin/true'

# --- getent (31.4)
run getent-help 'getent --help'
run getent-version 'getent --version | head -n 1'
run getent-passwd 'getent passwd root'
run getent-group 'getent group root'
run getent-notfound 'getent passwd nosuchuser-util-spec'
run getent-unknowndb 'getent nosuchdb'
run getent-hosts 'getent hosts 127.0.0.1'
run getent-ahosts 'getent ahosts 127.0.0.1'
run getent-services 'getent services 22/tcp'
run getent-initgroups 'getent initgroups root'
run getent-noenum 'getent initgroups'
run getent-s 'getent -s files passwd root'

# --- getconf (31.5)
run getconf-help 'getconf --help'
run getconf-version 'getconf --version | head -n 1'
run getconf-a 'getconf -a | wc -l; getconf -a | grep -e "^PAGESIZE" -e "^LONG_BIT" -e "^GNU_LIBC_VERSION"'
run getconf-values 'for n in PAGESIZE PAGE_SIZE LONG_BIT PATH GNU_LIBC_VERSION GNU_LIBPTHREAD_VERSION LFS_CFLAGS POSIX_V7_LP64_OFF64_CFLAGS; do printf "%s=[%s] rc=" $n "$(getconf $n)"; getconf $n >/dev/null; echo $?; done'
run getconf-nproc 'getconf _NPROCESSORS_ONLN | grep -c "^[1-9][0-9]*$"'
run getconf-unknown 'getconf NOSUCHNAME'
run getconf-pathvar-nopath 'getconf NAME_MAX'
run getconf-pathvar 'getconf NAME_MAX /'
run getconf-v 'getconf -v POSIX_V7_LP64_OFF64 LONG_BIT; getconf -v nosuchspec LONG_BIT'

# --- iconv (31.6)
printf 'ab\303\251\377cd' >"$T/bad.txt"
printf 'caf\303\251\n' >"$T/cafe.txt"
run iconv-help 'iconv --help'
run iconv-version 'iconv --version | head -n 1'
run iconv-illegal 'iconv -f UTF-8 -t UTF-16LE bad.txt > o16; r=$?; od -An -tx1 o16; exit $r'
run iconv-unconvertible 'iconv -f UTF-8 -t ASCII cafe.txt'
run iconv-c 'iconv -c -f UTF-8 -t ASCII cafe.txt'
run iconv-translit 'iconv -f UTF-8 -t ASCII//TRANSLIT cafe.txt'
run iconv-ignore 'iconv -f UTF-8 -t ASCII//IGNORE cafe.txt'
run iconv-unsupported 'iconv -f NOSUCH-CHARSET -t UTF-8 cafe.txt'
run iconv-nofile 'iconv -f UTF-8 -t ASCII nosuch.txt'
run iconv-o 'iconv -f UTF-8 -t ISO-8859-1 -o out.txt cafe.txt; od -An -tx1 out.txt'
run iconv-l 'iconv -l | wc -l; iconv -l | head -n 3'

# --- locale (31.7)
run locale-help 'locale --help'
run locale-version 'locale --version | head -n 1'
run locale-noenv 'locale'
run locale-lang 'LANG=C.UTF-8 LC_TIME=POSIX locale'
run locale-missing 'LANG=xx_YY.UTF-8 locale'
run locale-a 'locale -a | grep -e "^C" -e "^POSIX"'
run locale-a-v 'locale -a -v | sed -n "/C.utf8/,/^\$/p" | head -n 20'
run locale-m 'locale -m | wc -l; locale -m | grep -x UTF-8'
run locale-charmap 'locale charmap; LANG=C.UTF-8 locale charmap'
run locale-k 'locale -k decimal_point numeric-decimal-point-wc grouping'
run locale-ck 'locale -c -k LC_NUMERIC'
run locale-category 'locale LC_NUMERIC'
run locale-unknown 'locale nosuchname'

# --- localedef (31.8)
run localedef-help 'localedef --help'
run localedef-help-paths 'localedef --help | tail -n 12'
run localedef-version 'localedef --version | head -n 1'
run localedef-nosource 'localedef -i nosuchsource -f UTF-8 $T/ld1/'
run localedef-nocharmap 'localedef -i en_US -f NOSUCH-CHARMAP $T/ld2/'
run localedef-dir 'localedef -i en_US -f UTF-8 $T/ld3/; ls ld3 | sort; ls ld3/LC_MESSAGES'
run localedef-list 'localedef --list-archive | head -n 3'

# --- zic (31.9)
printf 'Rule\tT\t2000\tmax\t-\tMar\tlastSun\t1:00u\t1:00\tS\nRule\tT\t2000\tmax\t-\tOct\tlastSun\t1:00u\t0\t-\nZone\tTest/Zone\t1:00\tT\tT%%sT\nLink\tTest/Zone\tTest/Alias\n' >"$T/test.zi"
run zic-help 'zic --help'
run zic-version 'zic --version'
run zic-compile 'zic -d $T/z test.zi; find z | sort; head -c 5 z/Test/Zone | od -An -c; wc -c < z/Test/Zone'
run zic-slim 'zic -b slim -d $T/zs test.zi; head -c 5 zs/Test/Zone | od -An -c; wc -c < zs/Test/Zone'
run zic-nofile 'zic -d $T/z2 nosuch.zi'
run zic-tzdata 'head -n 1 /usr/share/zoneinfo/tzdata.zi; head -c 5 /usr/share/zoneinfo/Europe/London | od -An -c; wc -c < /usr/share/zoneinfo/Europe/London; for d in posix right; do [ -e /usr/share/zoneinfo/$d ] && echo $d present || echo $d absent; done'

# --- zdump (31.10)
run zdump-help 'zdump --help'
run zdump-version 'zdump --version'
run zdump-plain 'zdump Europe/London | sed "s/  .*/  <now>/"'
run zdump-v 'zdump -v -c 2025,2026 Europe/London'
run zdump-V 'zdump -V -c 2025,2026 Europe/London'
run zdump-i 'zdump -i -c 2025,2026 Europe/London'
run zdump-t 'zdump -v -t 1735689600,1767225600 Europe/London'
run zdump-test 'TZDIR=$T/z zdump -v -c 2025,2026 Test/Zone'

# --- tzselect (31.11)
run tzselect-help 'tzselect --help'
run tzselect-version 'tzselect --version'
printf '1\n' >"$T/one"
run tzselect-eof 'tzselect -n 3 -c +5130-00008' "$T/one"

# --- gencat (31.12)
printf '$set 1\n1 hello\n2 world\n' >"$T/msg.src"
run gencat-help 'gencat --help'
run gencat-version 'gencat --version | head -n 1'
run gencat-compile 'gencat msg.cat msg.src; wc -c < msg.cat; head -c 8 msg.cat | od -An -tx1'
run gencat-nofile 'gencat x.cat nosuch.src'

# --- pldd (31.13)
run pldd-help 'pldd --help'
run pldd-version 'pldd --version | head -n 1'
run pldd-nopid 'pldd'
run pldd-nosuchpid 'pldd 2147483646'
run pldd-self 'sleep 20 & p=$!; pldd $p | sed "s/^$p:/<pid>:/"; r=$?; kill $p; exit $r'

# --- not shipped by Liella (31.14)
run iconvconfig-help 'iconvconfig --help'
run devtools 'for c in sprof sotruss mtrace; do printf "%s " $c; command -v $c || echo missing; done; ls /etc/bindresvport.blacklist'
