#!/bin/bash
# LIELLA-UTILITIES-DESIGN probe 1: black-box observation of the reference utilities
# (options, output formats, exit codes, files, callers). Runs the programs only; never reads
# the source or the text of any libc or tzcode script (ldd, tzselect, mtrace, sotruss, locale-gen
# and update-locale are excluded from every content scan below).
set +e
unset LC_ALL LANGUAGE
export LANG=C.UTF-8
N=15
p() { echo "\$ $*"; timeout 60 "$@" 2>&1 | head -n "$N"; echo "rc=${PIPESTATUS[0]}"; }
sec() { echo; echo "### $1"; }
T=$(mktemp -d /tmp/lud-probe.XXXXXX)

sec system
N=12 p bash -c 'grep -E "^(PRETTY_NAME|VERSION_CODENAME)=" /etc/os-release; dpkg-query -W -f="\${Package} \${Version}\n" libc-bin libc-dev-bin locales tzdata dpkg debhelper libtool 2>&1'
N=30 p bash -c 'for m in ldconfig ldd getent getconf iconv locale localedef zic zdump tzselect tzfile locale-gen ld.so; do printf "%s: " $m; man -w $m 2>/dev/null | tr "\n" " "; echo; done; ls /usr/share/man/man8 2>/dev/null | wc -l'

sec owners
N=40 p bash -c 'for x in /usr/sbin/ldconfig /usr/sbin/ldconfig.real /usr/bin/ldd /usr/bin/getent /usr/bin/getconf /usr/bin/iconv /usr/sbin/iconvconfig /usr/bin/locale /usr/bin/localedef /usr/sbin/zic /usr/bin/zic /usr/bin/zdump /usr/bin/tzselect /usr/sbin/tzconfig /usr/sbin/locale-gen /usr/sbin/update-locale /usr/sbin/validlocale /usr/bin/pldd /usr/bin/gencat /usr/bin/sprof /usr/bin/sotruss /usr/bin/mtrace /usr/bin/ld.so /usr/bin/catchsegv /usr/sbin/nscd /usr/bin/rpcgen; do printf "%s: " $x; if [ -e $x ]; then o=$(dpkg -S $x 2>/dev/null || dpkg -S ${x#/usr} 2>/dev/null); echo "${o%%:*} | $(file -b $x | cut -c1-60)"; else echo absent; fi; done'

sec packages
N=80 p bash -c 'dpkg -L libc-bin | grep -v -E "^/usr/share/(doc|lintian|man|locale)|^/\.$" | sort'
N=20 p bash -c 'dpkg -L libc-dev-bin | grep -E "bin/" | sort; echo ---; dpkg -L locales | grep -E "bin/|^/etc" | sort; echo ---; dpkg -L tzdata | grep -E "bin/" | sort'

sec triggers
N=20 p bash -c 'ls /var/lib/dpkg/triggers/; echo ---; cat /var/lib/dpkg/triggers/ldconfig; echo ---; cd /var/lib/dpkg/info; echo triggers-files=$(ls *.triggers | wc -l); echo activate-ldconfig=$(grep -l -E "^activate(-noawait)? +ldconfig" *.triggers | wc -l); echo shlibs-files=$(ls *.shlibs 2>/dev/null | wc -l); echo interest-lines:; grep -h -E "^interest" *.triggers | sort | uniq -c | sort -rn | head -8'

sec dpkg-path-check
D=$T/dpkgroot
mkdir -p $D/var/lib/dpkg/info $D/var/lib/dpkg/updates $D/var/lib/dpkg/triggers $D/var/lib/dpkg/alternatives
touch $D/var/lib/dpkg/status $D/var/lib/dpkg/available
N=6 p env PATH=/usr/bin:/bin dpkg --force-not-root --root=$D --configure -a
N=6 p env PATH=/usr/bin:/bin dpkg --force-not-root --root=$D --unpack /nonexistent.deb
N=6 p env PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin dpkg --force-not-root --root=$D --unpack /nonexistent.deb

sec ldconfig
N=45 p ldconfig --help
N=2 p ldconfig --version
N=8 p bash -c 'ldconfig -p | head -3; echo ...; ldconfig -p | tail -2; ldconfig -p | grep -c "=>"'
N=12 p bash -c 'cat /etc/ld.so.conf; echo ---; ls /etc/ld.so.conf.d; echo ---; cat /etc/ld.so.conf.d/*.conf | grep -v "^#"'
N=12 p bash -c 'ldconfig -v -N -X 2>&1 | grep -E "^/"'
N=4 p ldconfig -X
N=4 p ldconfig -p -C $T/nonexistent.cache
mkdir -p $T/lr/lib
echo 'int f(void){return 1;}' > $T/x.c
for v in 1.2 1.9 1.10; do gcc -shared -fPIC -Wl,-soname,libfoo.so.1 -o $T/lr/lib/libfoo.so.$v $T/x.c; done
gcc -shared -fPIC -o $T/lr/lib/libnosoname.so.3 $T/x.c
gcc -shared -fPIC -Wl,-soname,libbar.so.2 -o $T/lr/lib/libbar.so.2.0.0 $T/x.c
ln -s libbar.so.2.0.0 $T/lr/lib/libbar.so
N=12 p ldconfig -n -v $T/lr/lib
N=12 p bash -c "cd $T/lr/lib && find . -type l -printf '%p -> %l\n' | sort"
mkdir -p $T/lr2/etc $T/lr2/usr/lib $T/lr2/opt/x
cp $T/lr/lib/libbar.so.2.0.0 $T/lr2/usr/lib/; cp $T/lr/lib/libfoo.so.1.2 $T/lr2/opt/x/
echo /opt/x > $T/lr2/etc/ld.so.conf
N=12 p unshare -r -m bash -c "ldconfig -r $T/lr2 -v 2>&1 | grep -v '^\s*$' | head -12; ldconfig -r $T/lr2 -p; ls -l $T/lr2/etc"

sec ldd
N=3 p ldd --version
N=20 p ldd --help
N=8 p ldd /usr/bin/true
N=4 p ldd /etc/hostname
N=4 p ldd /usr/sbin/ldconfig.real
N=25 p ldd -v /usr/bin/true
N=6 p ldd -u -r /usr/bin/true
N=10 p ldd /usr/bin/true /usr/bin/false
N=4 p ldd /nonexistent
N=4 p ldd
N=6 p env LD_TRACE_LOADED_OBJECTS=1 /usr/bin/true

sec getent
N=45 p getent --help
N=2 p getent --version
N=3 p getent passwd root
N=3 p getent passwd nosuchuser-lud
N=3 p getent nosuchdb x
N=4 p getent hosts localhost
N=8 p getent ahosts localhost
N=3 p getent -s files passwd root
N=3 p getent services 22
N=3 p getent group root
N=3 p getent initgroups root
N=3 p getent shadow root
N=2 p bash -c 'getent passwd | wc -l'
N=12 p bash -c 'grep -v "^#" /etc/nsswitch.conf | grep -v "^$"; ls /usr/lib/x86_64-linux-gnu/ | grep -E "^libnss_" | sed "s/\.so.*//" | sort -u | tr "\n" " "'

sec getconf
N=4 p getconf -a
N=2 p bash -c 'getconf -a | wc -l'
N=20 p bash -c 'for v in LONG_BIT PAGESIZE PAGE_SIZE _NPROCESSORS_ONLN ARG_MAX GNU_LIBC_VERSION GNU_LIBPTHREAD_VERSION CLK_TCK PATH LFS_CFLAGS POSIX_V7_LP64_OFF64_CFLAGS NOSUCH_VAR; do printf "%s=[%s] " $v "$(getconf $v 2>&1)"; getconf $v >/dev/null 2>&1; echo rc=$?; done'
N=3 p getconf NAME_MAX /
N=3 p getconf NAME_MAX
N=3 p getconf -v POSIX_V7_LP64_OFF64 LONG_BIT
N=3 p getconf -v NOSUCH LONG_BIT
N=6 p getconf
N=6 p getconf --help
N=3 p getconf --version

sec locale
N=15 p locale
N=10 p locale -a
N=2 p bash -c 'locale -m | wc -l'
N=2 p locale charmap
N=8 p locale -k LC_NUMERIC
N=3 p locale decimal_point
N=4 p locale -c LC_NUMERIC
N=30 p locale --help
N=6 p env LANG=xx_YY.UTF-8 locale
N=3 p locale nosuch_keyword
N=14 p locale -a -v

sec localedef
N=60 p localedef --help
N=4 p localedef --list-archive
N=6 p bash -c 'ls /usr/lib/locale; ls /usr/share/i18n; ls /usr/share/i18n/charmaps | wc -l; ls /usr/share/i18n/locales | wc -l; wc -l < /usr/share/i18n/SUPPORTED; ls /usr/share/locale/locale.alias /etc/locale.gen /etc/default/locale 2>&1'
mkdir -p $T/loc
N=6 p localedef -i en_US -f UTF-8 --no-archive $T/loc/en_US.UTF-8
N=20 p bash -c "cd $T/loc/en_US.UTF-8 && find . -type f -printf '%s %p\n' | sort -k2"
N=3 p env LOCPATH=$T/loc LC_ALL=en_US.UTF-8 locale charmap
N=4 p localedef -i nosuch_lud -f UTF-8 --no-archive $T/loc/x
N=4 p localedef -i en_US -f NOSUCHMAP --no-archive $T/loc/y

sec iconv
N=30 p iconv --help
N=2 p iconv --version
N=2 p bash -c 'iconv -l | wc -l; iconv -l | head -2'
N=3 p bash -c "printf 'caf\xc3\xa9\n' | iconv -f UTF-8 -t ASCII"
N=3 p bash -c "printf 'caf\xc3\xa9\n' | iconv -f UTF-8 -t ASCII//TRANSLIT"
N=3 p bash -c "printf 'caf\xc3\xa9\n' | iconv -c -f UTF-8 -t ASCII"
N=3 p bash -c "printf 'a\xffb\n' | iconv -f UTF-8 -t UTF-16LE | od -An -tx1"
N=3 p iconv -f NOSUCH -t UTF-8 /dev/null
N=3 p iconv -f UTF-8 -t UTF-8 /nonexistent
N=3 p bash -c 'ls /usr/lib/x86_64-linux-gnu/gconv | wc -l; ls /usr/lib/x86_64-linux-gnu/gconv | grep -c "\.so$"'

sec tz
N=3 p zic --version
N=25 p zic --help
N=3 p zdump --version
N=20 p zdump --help
N=20 p tzselect --help
N=3 p tzselect --version
N=3 p zdump Europe/London
N=8 p zdump -v -c 2024,2025 Europe/London
N=8 p zdump -i -c 2024,2025 Europe/London
N=6 p zdump -v -c 2024,2025 UTC
N=16 p bash -c 'Z=/usr/share/zoneinfo; head -c 5 $Z/Europe/London | od -c | head -1; ls -l $Z/Europe/London $Z/UTC | cut -c25-; head -1 $Z/tzdata.zi; echo zones=$(grep -c "^Z" $Z/tzdata.zi) links=$(grep -c "^L" $Z/tzdata.zi); ls $Z | wc -l; ls -d $Z/posix $Z/right 2>&1; ls $Z/*.tab $Z/leap* 2>&1 | tr "\n" " "; echo; find $Z -type f | wc -l'
mkdir -p $T/zi $T/zf
N=4 p zic -d $T/zi /usr/share/zoneinfo/tzdata.zi
N=4 p zic -b fat -d $T/zf /usr/share/zoneinfo/tzdata.zi
N=6 p bash -c "ls -l $T/zi/Europe/London $T/zf/Europe/London /usr/share/zoneinfo/Europe/London | cut -c25-; cd /usr/share/zoneinfo; n=0; s=0; sf=0; for f in \$(find . -type f ! -name '*.tab' ! -name '*.zi' ! -name 'leap*' ! -name '*.list' ! -name 'SECURITY' ! -path './posix/*' ! -path './right/*' | sort); do if [ -e $T/zi/\$f ]; then n=\$((n+1)); cmp -s $T/zi/\$f \$f && s=\$((s+1)); cmp -s $T/zf/\$f \$f && sf=\$((sf+1)); fi; done; echo compared=\$n same_default=\$s same_fat=\$sf"
N=3 p bash -c 'ls -l /etc/localtime | cut -c40-; cat /etc/timezone'

sec callers
cd /var/lib/dpkg/info
EX="--exclude=libc-bin.* --exclude=libc6* --exclude=libc-dev-bin.* --exclude=locales* --exclude=libc-l10n.*"
N=14 p bash -c "for u in ldconfig ldd getent getconf iconv iconvconfig localedef locale-gen update-locale zic zdump tzselect; do printf '%s maintscripts=' \$u; grep -l -w -E \"\$u\" $EX *.preinst *.postinst *.prerm *.postrm *.config 2>/dev/null | wc -l; done"
N=10 p bash -c "grep -h -o -w -E 'getent +[a-z]+' $EX *.preinst *.postinst *.prerm *.postrm *.config | sed 's/  */ /' | sort | uniq -c | sort -rn"
N=8 p bash -c "grep -h -o -E '(^|[^a-z.-])ldconfig( +-[A-Za-z]+)*' $EX *.preinst *.postinst *.prerm *.postrm *.config | sed 's/^[^l]*//' | sort | uniq -c | sort -rn"
cd /
DIRS="/usr/bin /usr/sbin /usr/share/initramfs-tools /usr/share/debhelper /usr/share/perl5 /usr/lib/python3 /usr/lib/python3.12 /etc /usr/share/dpkg /usr/lib/dpkg"
XF="--exclude=ldd --exclude=tzselect --exclude=mtrace --exclude=sotruss --exclude=xtrace --exclude=catchsegv --exclude=locale-gen --exclude=update-locale --exclude=validlocale --exclude=ldconfig --exclude-dir=i18n"
N=40 p bash -c "grep -rIhoE '(ldconfig +-[a-zA-Z]+|getconf +[A-Z_][A-Z_0-9]+|locale +(-[a-zA-Z]+|charmap)|iconv +-[a-z]|localedef +-[-a-z]+|zdump +-[a-z]|zic +-[a-z]|getent +[a-z]+|ldd +-?[a-z]*)' $XF $DIRS 2>/dev/null | sed 's/  */ /g' | sort | uniq -c | sort -rn | head -40"
N=12 p bash -c "grep -rIlE 'ldconfig.{0,8}-p' $XF $DIRS 2>/dev/null | head -12"
N=12 p bash -c "grep -rIlwE 'ldd' $XF /usr/share/initramfs-tools /usr/share/debhelper /usr/bin /usr/sbin /usr/share/perl5 2>/dev/null | head -12"

rm -rf "$T"
echo "### end"
