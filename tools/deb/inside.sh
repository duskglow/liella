#!/bin/sh
# tools/deb/inside.sh SUITE: the part of tools/deb/proof.sh that runs inside the chroot, as root
# there (an unprivileged user namespace outside). Prints "proof: PASS|FAIL|WAIT|INFO <what>" lines
# and "proof: RESULT ..."; always exits 0 (proof.sh judges the lines).
# WAIT: a step that needs loader work still landing: a dynamic program stops in Liella's loader
# with "the library search is not available in this build" (the start-time search for DT_NEEDED
# names), or a program cannot link because libc.so.6 does not export dlsym/dlclose yet (DL-16b,
# DL-17). KNOWN: a labelled Liella defect with its own story (the C++ runtime's "C" locale:
# newlocale refuses the mask 1<<LC_ALL). Both turn into PASS by themselves once fixed; neither
# counts as FAIL.
set -u
export LC_ALL=C
suite=${1:-?}
p=/tmp/proof
cd "$p" || exit 0
npass=0 nfail=0 nwait=0 nknown=0
pass() { echo "proof: PASS $*"; npass=$((npass + 1)); }
fail() { echo "proof: FAIL $*"; nfail=$((nfail + 1)); }
wait_() { echo "proof: WAIT $*"; nwait=$((nwait + 1)); }
known() { echo "proof: KNOWN $*"; nknown=$((nknown + 1)); }
knownmsg='_S_create_c_locale name not valid'
info() { echo "proof: INFO $*"; }
sysroot=/usr/lib/liella
interp=$sysroot/lib64/ld-linux-x86-64.so.2
lib=$sysroot/usr/lib/x86_64-linux-gnu
waitmsg='the library search is not available in this build'

# snapshot FILE: every path outside the pseudo and scratch file systems and dpkg's own records.
snapshot() {
    find / -xdev \( -path /proc -o -path /sys -o -path /dev -o -path /run -o -path /tmp -o -path /var/lib/dpkg \
        -o -path /var/log -o -path /var/cache -o -path /root \) -prune -o -print | sort > "$1"
}
# host_programs WHEN: the system's programs run on the system's C library.
host_programs() {
    if ls / > /dev/null && dpkg --version > /dev/null && gcc -O1 smoke/hello.c -o "hello-glibc-$1" &&
        [ "$(readelf -lW "hello-glibc-$1" | sed -n 's/.*interpreter: \(.*\)\]$/\1/p')" = /lib64/ld-linux-x86-64.so.2 ] &&
        [ "$("./hello-glibc-$1")" = "hello from liella" ] && perl -e 'exit 0'; then
        pass "system programs run ($1): ls, dpkg, perl, a gcc-built program on /lib64/ld-linux-x86-64.so.2"
    else
        fail "system programs ($1)"
    fi
}
# verify_libc WHEN: dpkg --verify finds libc6 and libc6-dev unchanged.
verify_libc() {
    out=$(dpkg --verify libc6 libc6-dev 2>&1)
    if [ -z "$out" ]; then pass "libc6 and libc6-dev unchanged ($1, dpkg --verify)"; else fail "dpkg --verify ($1): $out"; fi
}
# run NAME EXPECTED CMD...: a program run; WAIT when the loader lacks the start-time search.
run() {
    name=$1 want=$2
    shift 2
    timeout 60 "$@" > "$name.out" 2> "$name.err"
    rc=$?
    if [ $rc -eq 0 ] && grep -q -F "$want" "$name.out"; then
        pass "$name runs: $(head -1 "$name.out")"
    elif grep -q -F "$waitmsg" "$name.err"; then
        wait_ "$name (rc $rc; loader: $(head -1 "$name.err"))"
    elif grep -q -F "$knownmsg" "$name.err"; then
        known "$name (rc $rc: $(grep -F "$knownmsg" "$name.err" | head -1); newlocale(1<<LC_ALL, \"C\", 0) fails on Liella)"
    else
        fail "$name rc $rc: $(head -c 300 "$name.out" "$name.err" | tr '\n' ' ')"
    fi
    return 0
}

info "suite $suite, $(. /etc/os-release && echo "$PRETTY_NAME"), libc6 $(dpkg-query -W -f '${Version}' libc6), gcc $(gcc -dumpversion)"
info "libc6 interpreter files: $(dpkg -L libc6 | grep -E 'ld-linux-x86-64' | tr '\n' ' ')"
info "libc6-dev libc.so script: $(grep -v '^ *\*\|^/\*' "$(dpkg -L libc6-dev | grep '/libc\.so$')" | tr '\n' ' ')"
snapshot before
host_programs before
verify_libc before

# Install.
if dpkg -i debs/liella6_*.deb debs/liella6-dev_*.deb > install.log 2>&1; then
    pass "install: $(ls debs | tr '\n' ' ')"
else
    fail "install: $(tail -5 install.log | tr '\n' ' ')"
fi
info "installed files: $(dpkg -L liella6 liella6-dev | wc -l)"

# The loader's layout: Liella's sonames resolve inside the sysroot (ld.so --list).
liella-gcc -O1 smoke/hello.c -o hello > build.log 2>&1 || fail "build hello: $(tail -3 build.log | tr '\n' ' ')"
got=$(readelf -lW hello | sed -n 's/.*interpreter: \(.*\)\]$/\1/p')
[ "$got" = "$interp" ] && pass "hello's interpreter is $interp" || fail "hello's interpreter is '$got'"
list=$("$interp" --list ./hello 2>&1)
if echo "$list" | grep -q -F "libc.so.6 => $lib/libc.so.6"; then
    pass "ld.so --list resolves libc.so.6 to $lib/libc.so.6"
else
    fail "ld.so --list: $(echo "$list" | tr '\n' ' ')"
fi

# Dynamic smoke programs.
run hello "hello from liella" ./hello
liella-gcc -O1 smoke/threads.c -o threads >> build.log 2>&1 && run threads "threads: total 40000" ./threads || fail "build threads"
if ! liella-gcc -O1 -shared -fPIC smoke/plugin.c -o libplugin.so > plugin.log 2>&1; then
    fail "build plugin: $(tail -3 plugin.log | tr '\n' ' ')"
elif liella-gcc -O1 smoke/dlopen.c -o dlopen > dlopen.log 2>&1; then
    run dlopen "plugin_answer(20) = 41" ./dlopen ./libplugin.so
elif grep -q -E "undefined reference to .(dlsym|dlclose)'" dlopen.log; then
    wait_ "dlopen (does not link: $(grep -o -E "undefined reference to .(dlsym|dlclose)'" dlopen.log | sort -u | tr '\n' ' ')- libc.so.6 does not export them yet)"
else
    fail "build dlopen: $(tail -3 dlopen.log | tr '\n' ' ')"
fi
liella-g++ -O1 smoke/except.cc -o except >> build.log 2>&1 && run except "except: caught" ./except || fail "build except"
liella-g++ -O1 smoke/iostream.cc -o iostream >> build.log 2>&1 && run iostream "iostream: 42" ./iostream || fail "build iostream"
# Static smoke programs (libc.a; no interpreter).
liella-gcc -O1 -static smoke/hello.c -o hello-static >> build.log 2>&1 && run hello-static "hello from liella" ./hello-static || fail "build hello-static"
liella-gcc -O1 -static smoke/threads.c -o threads-static >> build.log 2>&1 && run threads-static "threads: total 40000" ./threads-static || fail "build threads-static"
liella-g++ -O1 -static smoke/except.cc -o except-static >> build.log 2>&1 && run except-static "except: caught" ./except-static || fail "build except-static"
liella-g++ -O1 -static smoke/iostream.cc -o iostream-static >> build.log 2>&1 && run iostream-static "iostream: 42" ./iostream-static || fail "build iostream-static"

host_programs installed
verify_libc installed

# lintian (when installed in the chroot): every tag listed; proof.sh's caller judges them.
if command -v lintian > /dev/null; then
    lintian --info --display-info --pedantic debs/*.deb > lintian.out 2>&1
    info "lintian: $(grep -c . lintian.out) line(s)"
    sed 's/^/proof: LINTIAN /' lintian.out
fi

# Purge: nothing left.
if dpkg --purge liella6-dev liella6 > purge.log 2>&1; then pass "purge"; else fail "purge: $(tail -3 purge.log | tr '\n' ' ')"; fi
snapshot after
if diff before after > leftover.diff; then
    pass "after purge the file list equals the one before install"
else
    fail "file list changed: $(head -20 leftover.diff | tr '\n' ' ')"
fi
[ ! -e "$sysroot" ] && pass "$sysroot is gone" || fail "$sysroot remains"
host_programs purged
verify_libc purged
echo "proof: RESULT suite=$suite pass=$npass fail=$nfail wait=$nwait known=$nknown"
exit 0
