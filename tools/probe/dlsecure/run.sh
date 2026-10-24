#!/bin/sh
# dlsecure: black-box probes of the reference loader in secure mode (DL-PROBE-SECURE, plan DL-0a).
# usage: run.sh SRCDIR SCRATCH   (SRCDIR = this directory; SCRATCH = a new directory on a file
# system that honours set-ID bits, made mode 0700 here). Needs: cc, ldconfig, readelf, unshare with
# subordinate ids (/etc/subuid, /etc/subgid) for the namespace cases. No root, no host state changed
# outside SCRATCH. Secure mode (AT_SECURE) comes from a set-group-ID copy of each probe whose group
# is a supplementary group of the caller (R14-18), or, inside a user + mount namespace, a group
# mapped from the caller's subordinate range. Output is the record (spec/behaviour/dynlink-secure.glibc.txt).
set -u
src=$(cd "$1" && pwd) || exit 2
mkdir -p "$2" && chmod 700 "$2" || exit 2
d=$(cd "$2" && pwd)
sgroup=$(id -Gn | tr ' ' '\n' | grep -vx "$(id -gn)" | head -1)
[ -n "$sgroup" ] || { echo "no supplementary group to make a set-group-ID file" >&2; exit 2; }
CC="cc -std=gnu17 -O1"
mkdir -p "$d/bin/lib" "$d/lc" "$d/pre" "$d/aud" "$d/zone" "$d/o" "$d/nsbin"
$CC -shared -fPIC -DLIBNAME='"libsgo"' -o "$d/bin/lib/libsgo.so" "$src/lib.c" || exit 3
$CC -shared -fPIC -DLIBNAME='"libpre"' -o "$d/pre/libpre.so" "$src/lib.c" || exit 3
$CC -shared -fPIC -DLIBNAME='"libsgc"' -Wl,-soname,libsgc.so.1 -o "$d/lc/libsgc.so.1" "$src/lib.c" || exit 3
$CC -shared -fPIC -o "$d/aud/libaud.so" "$src/audit.c" || exit 3
$CC -o "$d/bin/probe" "$src/probe.c" || exit 3
SGO="-DUSE_SGO -L$d/bin/lib -Wl,--no-as-needed -lsgo"
$CC $SGO -o "$d/bin/probe_needs" "$src/probe.c" || exit 3
$CC $SGO -Wl,--enable-new-dtags,-rpath,'$ORIGIN/lib' -o "$d/bin/probe_origin" "$src/probe.c" || exit 3
$CC $SGO -Wl,--disable-new-dtags,-rpath,'$ORIGIN/lib' -o "$d/bin/probe_origin_rpath" "$src/probe.c" || exit 3
for p in probe probe_needs probe_origin probe_origin_rpath; do
    cp "$d/bin/$p" "$d/bin/sg_$p" && chgrp "$sgroup" "$d/bin/sg_$p" && chmod 2755 "$d/bin/sg_$p" || exit 4
done
cp /usr/share/zoneinfo/Asia/Tokyo "$d/zone/myzone" || exit 3
echo "$d/lc" > "$d/lc.conf"
mkdir -p "$d/etc_ctl" "$d/etc_pre" "$d/etc_none" "$d/etc_empty" "$d/etc_garb" "$d/etc_t16" "$d/etc_thalf"
ldconfig -C "$d/etc_ctl/ld.so.cache" -f "$d/lc.conf" || exit 3
cp "$d/etc_ctl/ld.so.cache" "$d/etc_pre/ld.so.cache"
echo "$d/pre/libpre.so" > "$d/etc_pre/ld.so.preload"
: > "$d/etc_empty/ld.so.cache"
head -c 4096 /dev/zero | tr '\0' 'A' > "$d/etc_garb/ld.so.cache"
head -c 16 "$d/etc_ctl/ld.so.cache" > "$d/etc_t16/ld.so.cache"
size=$(wc -c < "$d/etc_ctl/ld.so.cache")
head -c $((size / 2)) "$d/etc_ctl/ld.so.cache" > "$d/etc_thalf/ld.so.cache"

KEEP='^(secure|sgo_value|object|env|tz|dlopen) '
norm() { sed -E "s|$d|<dir>|g; s/^ *[0-9]+:/<pid>:/; s/0x[0-9a-f]+/<hex>/g"; }
show() {
    grep -E "$KEEP" "$d/out" | norm
    n=$(grep -cvE "$KEEP" "$d/out")
    [ "$n" -gt 0 ] && echo "stdout other $n lines, first: $(grep -vE "$KEEP" "$d/out" | head -1 | norm)"
    n=$(wc -l < "$d/err")
    if [ "$n" -le 4 ]; then norm < "$d/err" | sed 's/^/stderr /'; else echo "stderr $n lines, first: $(head -1 "$d/err" | norm)"; fi
    echo "exit $rc"
}
# pair LABEL CWD "ENV=V ..." PROG ARGS...: the plain program, then its set-group-ID copy
pair() {
    label=$1; cwd=$2; envs=$3; prog=$4; shift 4
    for kind in plain secure; do
        p=$prog; [ $kind = secure ] && p=sg_$prog
        echo "== $label [$kind]"
        # timeout runs outside env -i: only the probe sees the test environment
        (cd "$cwd" && timeout 20 env -i PATH=/usr/bin:/bin $envs "$d/bin/$p" "$@") > "$d/out" 2> "$d/err"
        rc=$?; show
    done
}
# ns LABEL ETCDIR SECURE ARGS...: bin/probe in a user + mount namespace with ETCDIR bound over /etc
ns() {
    label=$1; etc=$2; sec=$3; shift 3
    echo "== ns $label [$( [ "$sec" = 1 ] && echo secure || echo plain )]"
    env -i PATH=/usr/bin:/bin timeout 30 unshare --map-root-user --map-users=auto --map-groups=auto -m sh -c '
        d=$1; etc=$2; sec=$3; shift 3; p=$d/bin/probe
        if [ "$sec" = 1 ]; then
            mount -t tmpfs none "$d/nsbin" && cp "$d/bin/probe" "$d/nsbin/probe" &&
                chgrp 1 "$d/nsbin/probe" && chmod 2755 "$d/nsbin/probe" || exit 90
            p=$d/nsbin/probe
        fi
        mount --bind "$etc" /etc || exit 91
        cd / && unset PWD OLDPWD && exec "$p" "$@"' _ "$d" "$d/$etc" "$sec" "$@" > "$d/out" 2> "$d/err"
    rc=$?; show
}

echo "# dlsecure record: $(ldd --version | head -1); kernel $(uname -r)"
echo "setgid copies: mode $(stat -c %a "$d/bin/sg_probe") group is a supplementary group of the caller, not its real group"
for p in probe_origin probe_origin_rpath; do
    echo "$p: $(readelf -d "$d/bin/$p" | grep -E 'RUNPATH|RPATH' | sed -E 's/^ *0x[0-9a-f]+ +//')"
done

pair "baseline" / "" probe env
pair "environment residue" / "LD_BIND_NOW=1 LD_BIND_NOT=1 LD_DYNAMIC_WEAK=1 LD_WARN=1 LD_VERBOSE=1 LD_HWCAP_MASK=0 LD_ORIGIN_PATH=$d/o LD_PROFILE=libnonexistent.so LD_PROFILE_OUTPUT=$d/o LD_LIBRARY_PATH=/nonexistent LD_DEBUG_OUTPUT=$d/o/dbg LD_ASSUME_KERNEL=6.0.0 LD_USE_LOAD_BIAS=0 LD_PRELOAD= LD_AUDIT= LD_DEBUG= GLIBC_TUNABLES=glibc.malloc.tcache_count=0 TZ=UTC TZDIR=$d/o LOCPATH=$d/o NLSPATH=$d/o GCONV_PATH=$d/o MALLOC_CHECK_=0 MALLOC_PERTURB_=0 MALLOC_ARENA_MAX=1 MALLOC_TRACE=$d/o/mtrace HOSTALIASES=$d/o LOCALDOMAIN=x RES_OPTIONS=ndots:1 RESOLV_HOST_CONF=$d/o TMPDIR=$d/o" probe env

pair "LD_PRELOAD absolute untrusted" / "LD_PRELOAD=$d/pre/libpre.so" probe env
pair "LD_PRELOAD bare name, trusted dir, no set-ID bit" / "LD_PRELOAD=libz.so.1" probe env
pair "LD_PRELOAD absolute, trusted dir, no set-ID bit" / "LD_PRELOAD=/lib/x86_64-linux-gnu/libz.so.1" probe env
pair "LD_PRELOAD bare name, missing" / "LD_PRELOAD=nonexistent.so" probe env
pair "LD_AUDIT absolute untrusted module" / "LD_AUDIT=$d/aud/libaud.so" probe env
pair "LD_AUDIT bare name, trusted dir, not a module" / "LD_AUDIT=libz.so.1" probe env

pair "DT_NEEDED, no search path" / "" probe_needs
pair "LD_LIBRARY_PATH absolute" / "LD_LIBRARY_PATH=$d/bin/lib" probe_needs
pair "LD_LIBRARY_PATH empty elements, cwd holds the library" "$d/bin/lib" "LD_LIBRARY_PATH=:" probe_needs
pair "LD_LIBRARY_PATH relative element" "$d/bin" "LD_LIBRARY_PATH=lib" probe_needs
pair "RUNPATH \$ORIGIN/lib, untrusted program directory" / "" probe_origin
pair "RPATH \$ORIGIN/lib, untrusted program directory" / "" probe_origin_rpath

pair "dlopen \$ORIGIN token in the name" / "" probe dlopen '$ORIGIN/lib/libsgo.so'
pair "dlopen relative name with a slash" "$d/bin" "" probe dlopen lib/libsgo.so
pair "dlopen absolute untrusted" / "" probe dlopen "$d/bin/lib/libsgo.so"

pair "LD_DEBUG=libs" / "LD_DEBUG=libs" probe
pair "LD_DEBUG=libs with LD_DEBUG_OUTPUT" / "LD_DEBUG=libs LD_DEBUG_OUTPUT=$d/o/dbgout" probe
echo "LD_DEBUG_OUTPUT files: $(ls "$d/o" | grep -c '^dbgout')"
pair "LD_SHOW_AUXV=1" / "LD_SHOW_AUXV=1" probe
pair "LD_SHOW_AUXV empty" / "LD_SHOW_AUXV=" probe
pair "LD_TRACE_LOADED_OBJECTS empty" / "LD_TRACE_LOADED_OBJECTS=" probe
pair "LD_TRACE_LOADED_OBJECTS=1" / "LD_TRACE_LOADED_OBJECTS=1" probe
pair "GLIBC_TUNABLES" / "GLIBC_TUNABLES=glibc.malloc.check=3" probe

for tz in Asia/Tokyo :Asia/Tokyo JST-9 /usr/share/zoneinfo/Asia/Tokyo :/usr/share/zoneinfo/Asia/Tokyo \
          "$d/zone/myzone" ":$d/zone/myzone" Asia/../Asia/Tokyo "../../../..$d/zone/myzone"; do
    pair "TZ=$(echo "$tz" | sed "s|$d|<dir>|g")" / "TZ=$tz" probe tz
done
pair "TZDIR=<dir>/zone TZ=myzone" / "TZDIR=$d/zone TZ=myzone" probe tz

ns "private cache, control" etc_ctl 0 dlopen libsgc.so.1
ns "private cache, control" etc_ctl 1 dlopen libsgc.so.1
ns "no cache file" etc_none 0 dlopen libsgc.so.1
ns "empty cache file" etc_empty 0 dlopen libsgc.so.1
ns "garbage cache file" etc_garb 0 dlopen libsgc.so.1
ns "cache cut to 16 bytes" etc_t16 0 dlopen libsgc.so.1
ns "cache cut in half" etc_thalf 0 dlopen libsgc.so.1
ns "garbage cache file" etc_garb 1 dlopen libsgc.so.1
ns "/etc/ld.so.preload names an untrusted absolute path" etc_pre 0 env
ns "/etc/ld.so.preload names an untrusted absolute path" etc_pre 1 env
exit 0
