#!/bin/sh
# tools/deb/check.sh DIR: check the reference packages in DIR (tools/deb/build.sh's output) without
# installing them. No spec clause: release packaging (records/pending/RELEASE-PACKAGING-DEB-1.toml).
#
# 1. Coexistence: every path a package installs is inside the sysroot /usr/lib/liella, or is the
#    wrapper (/usr/bin/liella-gcc, liella-g++), its manual page, or the package's own
#    /usr/share/doc directory; the shared parents (/usr, /usr/bin, ...) only as directories. So no
#    file of the system's C library (libc6, libc6-dev) can be replaced. Owners are root:root.
# 2. No maintainer scripts, triggers, conffiles or shlibs: installing changes nothing outside the
#    files (no ld.so.conf, no cache).
# 3. The wrapper, from the unpacked packages: a dynamic program's interpreter is exactly Liella's
#    loader and it needs libc.so.6 with no RPATH/RUNPATH; static programs (C, threads, C++ with an
#    exception) build and run and have no interpreter.
# Prints "check: PASS" or one "check: FAIL ..." line per problem (exit 1).
set -eu
export LC_ALL=C
[ $# -eq 1 ] || { echo "usage: tools/deb/check.sh DIR" >&2; exit 2; }
dir=$(cd "$1" && pwd)
here=$(cd "$(dirname "$0")" && pwd)
interp=/usr/lib/liella/lib64/ld-linux-x86-64.so.2
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
fails=0
fail() {
    echo "check: FAIL $*"
    fails=$((fails + 1))
}

debs=$(find "$dir" -maxdepth 1 -name 'liella6*_amd64.deb' | sort)
[ -n "$debs" ] || { echo "check: FAIL no liella6*.deb in $dir"; exit 1; }
seen=""
for deb in $debs; do
    pkg=$(dpkg-deb -f "$deb" Package)
    seen="$seen $pkg"
    case $pkg in
        liella6 | liella6-dev) ;;
        *) fail "$deb: package name $pkg is not liella6 or liella6-dev" ;;
    esac
    dpkg-deb --fsys-tarfile "$deb" | tar -tv > "$tmp/list"
    awk '$2 != "root/root" { print $0 }' "$tmp/list" > "$tmp/owners"
    [ ! -s "$tmp/owners" ] || fail "$deb: entries not owned by root/root: $(head -1 "$tmp/owners")"
    # Each entry: type (first letter of the mode) and path (field 6; symlinks have "-> target").
    awk '{ print substr($1, 1, 1), $6 }' "$tmp/list" | while read -r type path; do
        case $path in
            ./usr/lib/liella/*) ;;
            ./usr/share/doc/"$pkg"/*) ;;
            ./usr/bin/liella-gcc | ./usr/bin/liella-g++ | ./usr/share/man/man1/liella-gcc.1.gz | ./usr/share/man/man1/liella-g++.1.gz)
                [ "$pkg" = liella6-dev ] || echo "check: FAIL $deb: $path belongs in liella6-dev" ;;
            ./ | ./usr/ | ./usr/lib/ | ./usr/bin/ | ./usr/share/ | ./usr/share/doc/ | ./usr/share/man/ | ./usr/share/man/man1/)
                [ "$type" = d ] || echo "check: FAIL $deb: $path is not a directory" ;;
            *) echo "check: FAIL $deb: path outside the Liella sysroot: $path" ;;
        esac
    done > "$tmp/paths"
    if [ -s "$tmp/paths" ]; then
        cat "$tmp/paths"
        fails=$((fails + $(wc -l < "$tmp/paths")))
    fi
    dpkg-deb --ctrl-tarfile "$deb" | tar -t | sort > "$tmp/ctrl"
    extra=$(grep -v -x -e './' -e './control' -e './md5sums' "$tmp/ctrl" || true)
    [ -z "$extra" ] || fail "$deb: control members besides control and md5sums: $(echo "$extra" | tr '\n' ' ')"
    dpkg-deb -x "$deb" "$tmp/root"
done
case "$seen" in
    *liella6-dev*) ;;
    *) fail "liella6-dev is missing from $dir" ;;
esac

# 3. The wrapper.
cc="$tmp/root/usr/bin/liella-gcc"
cxx="$tmp/root/usr/bin/liella-g++"
export LIELLA_SYSROOT="$tmp/root/usr/lib/liella"
if [ -x "$cc" ]; then
    if "$cc" -O1 "$here/smoke/hello.c" -o "$tmp/hello-dyn" > "$tmp/log" 2>&1; then
        got=$(readelf -lW "$tmp/hello-dyn" | sed -n 's/.*Requesting program interpreter: \(.*\)\]$/\1/p')
        [ "$got" = "$interp" ] || fail "wrapper: program interpreter is ${got:-none}, not Liella's loader $interp"
        needed=$(readelf -dW "$tmp/hello-dyn" | sed -n 's/.*(NEEDED).*\[\(.*\)\]$/\1/p' | tr '\n' ' ')
        [ "$needed" = "libc.so.6 " ] || fail "wrapper: hello needs '$needed', not libc.so.6 alone"
        ! readelf -dW "$tmp/hello-dyn" | grep -q -E '\((RPATH|RUNPATH)\)' || fail "wrapper: hello has an RPATH or RUNPATH"
    else
        fail "wrapper: dynamic hello does not link: $(tail -3 "$tmp/log" | tr '\n' ' ')"
    fi
    # run_static NAME COMPILER SOURCE EXPECTED: a static program builds, has no interpreter, prints EXPECTED.
    run_static() {
        if ! "$2" -O1 -static "$here/smoke/$3" -o "$tmp/$1" > "$tmp/log" 2>&1; then
            fail "wrapper: static $1 does not link: $(tail -3 "$tmp/log" | tr '\n' ' ')"
            return 0
        fi
        ! readelf -lW "$tmp/$1" | grep -q 'program interpreter' || fail "wrapper: static $1 has a program interpreter"
        timeout 60 "$tmp/$1" > "$tmp/$1.out" 2>&1 || fail "wrapper: static $1 exits $?: $(head -c 200 "$tmp/$1.out")"
        grep -q -F "$4" "$tmp/$1.out" || fail "wrapper: static $1 printed '$(head -c 200 "$tmp/$1.out")', not '$4'"
    }
    run_static hello "$cc" hello.c "hello from liella"
    run_static threads "$cc" threads.c "threads: total 40000, thread-local sum 40000"
    if command -v g++ > /dev/null; then
        run_static except "$cxx" except.cc "except: caught: thrown at depth 2"
    else
        echo "check: skip static except (no g++ on this host)"
    fi
else
    fail "no liella-gcc in liella6-dev"
fi

if [ "$fails" -ne 0 ]; then
    echo "check: $fails problem(s)"
    exit 1
fi
echo "check: PASS ($(echo "$debs" | wc -l) packages)"
