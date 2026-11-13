#!/bin/sh
# validation/smoke/smoke.sh: build real programs from their Debian source packages against Liella, run their
# own test suites, and run the same suites on a build against the host's C library (the reference).
# See validation/smoke/README.md. Results: <work>/results.tsv and <work>/results/<program>.<side>.*.
#
#   validation/smoke/smoke.sh [--work DIR] [--products DIR] [--jobs N] [--timeout SECS] [PROGRAM...]
#   validation/smoke/smoke.sh --list
#
# Default PROGRAMs: all of programs.sh, in the order of SMOKE_PROGRAMS below. Liella's products are read from
# --products (default target/liella; build them first with `LIELLA_FULL_PROFILE=1 cargo xtask so`). Sources
# come from deb.debian.org only (suite trixie), through a private apt configuration in <work>/apt whose
# trust anchor is the debian-archive-keyring package fetched over https from deb.debian.org; apt checks the
# suite's signature and every file's hash. Nothing needs root, nothing is installed.
#
# Exit status: 0 when every requested program was processed (failures of a program's tests are results, not
# errors), 1 when a Liella build produced an object that check-elf.sh rejects (a fallback to the system's C
# library: the run cannot be trusted), 2 on a usage or setup error.
set -u
here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/../.." && pwd)
work=$root/target/smoke
products=$root/target/liella
J=$(nproc)
limit=5400
suite=trixie
mirror=https://deb.debian.org/debian
SMOKE_PROGRAMS="zlib xz_utils sqlite3 grep sed diffutils findutils coreutils gawk make bash"

. "$here/programs.sh"

progs=""
while [ $# -gt 0 ]; do
    case $1 in
        --work) work=$2; shift 2 ;;
        --products) products=$2; shift 2 ;;
        --jobs) J=$2; shift 2 ;;
        --timeout) limit=$2; shift 2 ;;
        --list) echo "$SMOKE_PROGRAMS"; exit 0 ;;
        -h | --help) sed -n '2,18p' "$0"; exit 0 ;;
        -*) echo "smoke: unknown option $1" >&2; exit 2 ;;
        *) progs="$progs $(printf '%s' "$1" | tr '-' '_')"; shift ;;
    esac
done
[ -n "$progs" ] || progs=$SMOKE_PROGRAMS
mkdir -p "$work" || exit 2
work=$(cd "$work" && pwd)
products=$(cd "$products" 2>/dev/null && pwd) || { echo "smoke: no products directory" >&2; exit 2; }
ldso=$products/ld-linux-x86-64.so.2
sys=$work/sysroot
export J

# bypath IN OUT: a copy of the shared object IN whose DT_SONAME entry is retagged DT_CHECKSUM (loaders ignore
# it), so an object linked against OUT names it by OUT's path (the method of conformance/run.sh).
bypath() {
    off=$(readelf -Wd "$1" | sed -n 's/^Dynamic section at offset \(0x[0-9a-f]*\) .*/\1/p')
    idx=$(readelf -Wd "$1" | awk '$1 ~ /^0x/ { if ($2 == "(SONAME)") { print n + 0; exit } n++ }')
    [ -n "$off" ] && [ -n "$idx" ] && cp "$1" "$2" &&
        printf '\370\375\377\157\000\000\000\000' | dd of="$2" bs=1 seek=$((off + 16 * idx)) conv=notrunc 2>/dev/null &&
        ! readelf -Wd "$2" | grep -qF '(SONAME)'
}

# setup: the link-time directory liella-cc reads (fresh each run, so it always matches the products).
setup() {
    for f in libc.so.6 libm.so.6 ld-linux-x86-64.so.2; do
        [ -f "$products/$f" ] || { echo "smoke: $products/$f is missing (cargo xtask so)" >&2; return 1; }
    done
    rm -rf "$sys" && mkdir -p "$sys" "$work/bin" "$work/results" || return 1
    bypath "$products/libc.so.6" "$sys/libc-by-path.so" || { echo "smoke: cannot make the by-path libc" >&2; return 1; }
    bypath "$products/libm.so.6" "$sys/libm.so" || { echo "smoke: cannot make the by-path libm" >&2; return 1; }
    for a in libpthread libdl librt libutil libanl libresolv; do
        if [ -f "$products/$a.a" ]; then cp "$products/$a.a" "$sys/$a.a"; else ar rc "$sys/$a.a"; fi
    done
    nonshared=$(gcc -print-file-name=libc_nonshared.a)
    case $nonshared in /*) ;; *) echo "smoke: the host has no libc_nonshared.a" >&2; return 1 ;; esac
    printf 'GROUP ( "%s" "%s" AS_NEEDED ( "%s" ) )\n' "$sys/libc-by-path.so" "$nonshared" "$ldso" > "$sys/libc.so"
    cp "$here/liella-cc" "$work/bin/liella-cc" && chmod +x "$work/bin/liella-cc" &&
        ln -sf liella-cc "$work/bin/liella-c++" || return 1
    if [ -d "$root/.git" ] && rev=$(git -C "$root" rev-parse --short=12 HEAD 2>/dev/null); then :; else rev=unknown; fi
    profile=$(cat "$products/.profile" 2>/dev/null || echo unknown)
    printf 'tree %s, products %s (profile %s), host C library %s\n' "$rev" "$products" "$profile" \
        "$(ldd --version 2>&1 | sed -n 1p)" > "$work/results/run-info.txt"
}

# The private apt configuration: deb-src lines only, the Debian archive key as the only trust anchor.
apt_setup() {
    a=$work/apt
    [ -f "$a/ready" ] && return 0
    rm -rf "$a" && mkdir -p "$a/empty" "$a/state/lists/partial" "$a/cache/archives/partial" "$a/key" || return 1
    deb=$(curl -fsSL "$mirror/pool/main/d/debian-archive-keyring/" | grep -o 'debian-archive-keyring_[^"<>]*_all\.deb' |
        sort -u -V | tail -1)
    [ -n "$deb" ] || { echo "smoke: cannot list debian-archive-keyring on $mirror" >&2; return 1; }
    curl -fsSL -o "$a/key/$deb" "$mirror/pool/main/d/debian-archive-keyring/$deb" &&
        dpkg-deb -x "$a/key/$deb" "$a/key/root" || return 1
    k=$(find "$a/key/root" -name 'debian-archive-keyring.gpg' -o -name 'debian-archive-keyring.pgp' | sed -n 1p)
    [ -n "$k" ] || { echo "smoke: no keyring in $deb" >&2; return 1; }
    cp "$k" "$a/debian-archive-keyring.gpg" || return 1
    printf 'deb-src [signed-by=%s] %s %s main\n' "$a/debian-archive-keyring.gpg" "$mirror" "$suite" > "$a/sources.list"
    : > "$a/status"
    apt_get update || return 1
    echo "$deb" > "$a/ready"
}
apt_get() {
    a=$work/apt
    apt-get -q -o Dir::Etc::SourceList="$a/sources.list" -o Dir::Etc::SourceParts="$a/empty" \
        -o Dir::Etc::Parts="$a/empty" -o Dir::Etc::PreferencesParts="$a/empty" -o Dir::Etc::Preferences="$a/empty/none" \
        -o Dir::Etc::Trusted="$a/empty/none" -o Dir::Etc::TrustedParts="$a/empty" \
        -o Dir::State="$a/state" -o Dir::State::status="$a/status" -o Dir::Cache="$a/cache" \
        -o Debug::NoLocking=1 -o APT::Sandbox::User="$(id -un)" -o Acquire::Languages=none "$@"
}

# fetch SRCPKG: unpack the source package once; prints its directory.
fetch() {
    d=$work/src/$1
    if [ ! -f "$d/.ready" ]; then
        rm -rf "$d" && mkdir -p "$d" || return 1
        (cd "$d" && apt_get source "$1") > "$d.log" 2>&1 || { echo "smoke: apt-get source $1 failed (see $d.log)" >&2; return 1; }
        touch "$d/.ready"
    fi
    find "$d" -mindepth 1 -maxdepth 1 -type d | sed -n 1p
}

# count KIND LOG DIR OUT: write "total pass fail skip xfail error" for one test run to OUT, and the names of the
# failing tests (one per line) to OUT.failed.
count() {
    kind=$1; log=$2; dir=$3; out=$4
    : > "$out.failed"
    case $kind in
        automake)
            find "$dir" -name test-suite.log -type f | sort | while IFS= read -r f; do sed -n 1,12p "$f"; done |
                awk '/^# TOTAL:/ { t += $3 } /^# PASS:/ { p += $3 } /^# FAIL:/ { f += $3 } /^# SKIP:/ { s += $3 }
                     /^# XFAIL:/ { x += $3 } /^# ERROR:/ { e += $3 } /^# XPASS:/ { f += $3 }
                     END { print t + 0, p + 0, f + 0, s + 0, x + 0, e + 0 }' > "$out"
            grep -E '^(FAIL|ERROR|XPASS): ' "$log" | sort -u > "$out.failed" ;;
        gawk)
            n=$(sed -n 's/^\([0-9][0-9]*\) TESTS* FAILED.*/\1/p' "$log" | tail -1)
            t=$(grep -c '^[a-zA-Z0-9_]*$' "$log")
            if grep -q 'ALL TESTS PASSED' "$log"; then n=0; fi
            [ -n "$n" ] || n=1
            echo "$t $((t - n)) $n 0 0 0" > "$out"
            sed -n '/TESTS* FAILED/,$p' "$log" | grep -v 'FAILED' | sort -u > "$out.failed" ;;
        make)
            if grep -q 'No Failures' "$log"; then f=0; else f=$(sed -n 's/^\([0-9][0-9]*\) Tests* in [0-9]* Categor.* Failed.*/\1/p' "$log" | tail -1); fi
            t=$(grep -c '\.\.\.\.\.\.' "$log")
            [ -n "$f" ] || f=1
            echo "$t $((t - f)) $f 0 0 0" > "$out"
            grep -E '\.\.\.\.\.\. *(FAILED|Error)' "$log" | sort -u > "$out.failed" ;;
        bash)
            # A run-* test passes when its section of output is the reference build's; the reference build
            # itself counts its sections as passed (its own diffs are the suite's expected noise on this host).
            awk '/^run-/ { name = $1; next } name != "" { print name "\t" $0 }' "$log" | sort > "$out.sections"
            t=$(grep -c '^run-' "$log")
            if [ -f "$5" ]; then
                diff "$5" "$out.sections" | sed -n 's/^[<>] \([^\t]*\)\t.*/\1/p' | sort -u > "$out.failed"
            fi
            f=$(wc -l < "$out.failed")
            echo "$t $((t - f)) $f 0 0 0" > "$out" ;;
        rc)
            if [ "$5" = 0 ]; then echo "1 1 0 0 0 0" > "$out"; else echo "1 0 1 0 0 0" > "$out"; echo "suite (exit $5)" > "$out.failed"; fi ;;
    esac
}

# run_one NAME SIDE SRC: build NAME on SIDE (glibc | liella) and run its tests; appends to results.tsv.
run_one() {
    name=$1; side=$2; src=$3
    b=$work/build/$name/$side
    r=$work/results/$name.$side
    rm -rf "$b" && mkdir -p "$work/build/$name" && cp -a "$src" "$b" || return 1
    if [ "$side" = liella ]; then cc_=$work/bin/liella-cc; cxx_=$work/bin/liella-c++; else cc_=gcc; cxx_=g++; fi
    eval "kind=\${${name}_kind}"
    noregen="ACLOCAL=true AUTOCONF=true AUTOMAKE=true AUTOHEADER=true MAKEINFO=true"
    status=ok; check=-
    if (cd "$b" && CC=$cc_ CXX=$cxx_ MAKEFLAGS=$noregen LIELLA_SMOKE_SYSROOT=$sys LIELLA_SMOKE_LDSO=$ldso \
            timeout "$limit" sh -c '. "$1"; "$2"' sh "$here/programs.sh" "${name}_build") > "$r.build.log" 2>&1; then
        built=yes
    else
        built=no; status="build failed (exit $?)"
    fi
    if [ "$side" = liella ]; then
        if "$here/check-elf.sh" "$ldso" "$sys" "$b" > "$r.check.log" 2>&1; then check=ok; else check=FAIL; fi
    fi
    counts="- - - - - -"
    if [ $built = yes ] && [ "$check" != FAIL ]; then
        eval "skip=\${${name}_skip:-}"
        if [ -n "$skip" ]; then
            status="tests skipped: $skip"
        else
            (cd "$b" && MAKEFLAGS=$noregen LIELLA_SMOKE_SYSROOT=$sys LIELLA_SMOKE_LDSO=$ldso timeout "$limit" sh -c '. "$1"; "$2"' sh "$here/programs.sh" "${name}_test") \
                > "$r.test.log" 2>&1
            trc=$?
            [ $trc -eq 124 ] && status="test suite hit the ${limit}s hang guard"
            count "$kind" "$r.test.log" "$b" "$r.count" "$( [ "$kind" = bash ] && echo "$work/results/$name.glibc.count.sections" || echo $trc)"
            counts=$(cat "$r.count")
        fi
    fi
    printf '%s\t%s\t%s\t%s\t%s\t%s\n' "$name" "$side" "$built" "$check" "$(echo "$counts" | tr ' ' '\t')" "$status" \
        >> "$work/results.tsv"
    echo "smoke: $name $side: built=$built check=$check total/pass/fail/skip/xfail/error=$counts $status"
    [ "$check" != FAIL ]
}

# selftest: the harness's own check (no download). A program and a shared object built by liella-cc must pass
# check-elf.sh, run on Liella's loader and list Liella's libc; the same program built by the system compiler
# must be rejected. A wrapper or check that lets a build fall back to the system's C library fails here.
selftest() {
    t=$work/selftest
    rm -rf "$t" && mkdir -p "$t/ok" "$t/bad" || return 1
    e=0
    env LIELLA_SMOKE_SYSROOT="$sys" LIELLA_SMOKE_LDSO="$ldso" "$work/bin/liella-cc" -O1 -shared -fPIC \
        "$here/selftest/plugin.c" -o "$t/ok/libplugin.so" > "$t/build.log" 2>&1 &&
    env LIELLA_SMOKE_SYSROOT="$sys" LIELLA_SMOKE_LDSO="$ldso" "$work/bin/liella-cc" -O1 "$here/selftest/hello.c" \
        -lm -lpthread -o "$t/ok/hello" >> "$t/build.log" 2>&1 || { echo "smoke-selftest: FAIL liella-cc does not build (see $t/build.log)"; return 1; }
    gcc -O1 "$here/selftest/hello.c" -lm -o "$t/bad/hello" >> "$t/build.log" 2>&1 || { echo "smoke-selftest: FAIL gcc does not build"; return 1; }
    "$here/check-elf.sh" "$ldso" "$sys" "$t/ok" > "$t/check-ok.log" 2>&1 || { echo "smoke-selftest: FAIL check-elf rejects liella-cc's build:"; cat "$t/check-ok.log"; e=1; }
    if "$here/check-elf.sh" "$ldso" "$sys" "$t/bad" > "$t/check-bad.log" 2>&1; then
        echo "smoke-selftest: FAIL check-elf accepts a build against the system's C library"; e=1
    fi
    out=$("$t/ok/hello" 2 2>&1); rc=$?
    [ $rc -eq 0 ] && [ "$out" = "hello 1.414214" ] || { echo "smoke-selftest: FAIL hello on Liella: exit $rc, output: $out"; e=1; }
    "$ldso" --list "$t/ok/hello" > "$t/list.log" 2>&1
    grep -qF "$sys/libc-by-path.so" "$t/list.log" || { echo "smoke-selftest: FAIL Liella's loader does not list Liella's libc:"; cat "$t/list.log"; e=1; }
    [ $e -eq 0 ] && echo "smoke-selftest: PASS"
    return $e
}

setup || exit 2
if [ "$progs" = " selftest" ]; then selftest; exit $?; fi
apt_setup || exit 2
[ -f "$work/results.tsv" ] ||
    printf 'program\tside\tbuilt\tcheck\ttotal\tpass\tfail\tskip\txfail\terror\tstatus\n' > "$work/results.tsv"
bad=0
for p in $progs; do
    eval "srcpkg=\${${p}_src:-}"
    [ -n "$srcpkg" ] || { echo "smoke: unknown program $p (--list)" >&2; exit 2; }
    src=$(fetch "$srcpkg") && [ -n "$src" ] || { echo "smoke: $p: no source" >&2; continue; }
    run_one "$p" glibc "$src"
    run_one "$p" liella "$src" || bad=1
    if [ -f "$work/results/$p.glibc.count.failed" ] && [ -f "$work/results/$p.liella.count.failed" ]; then
        sort -u "$work/results/$p.glibc.count.failed" > "$work/results/$p.g.tmp"
        sort -u "$work/results/$p.liella.count.failed" | comm -13 "$work/results/$p.g.tmp" - > "$work/results/$p.liella-only.failed"
        rm -f "$work/results/$p.g.tmp"
        echo "smoke: $p: $(wc -l < "$work/results/$p.liella-only.failed") failure(s) on Liella only ($work/results/$p.liella-only.failed)"
    fi
done
[ $bad -eq 0 ] || echo "smoke: FAIL: a Liella build produced an object not linked against Liella (see results/*.check.log)"
exit $bad
