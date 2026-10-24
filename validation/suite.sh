#!/bin/bash
# LAB-ONLY: this script drives the project's own test hosts and is not needed to build or test Liella.
# Build one GNU package against one C library and run its test suite (validation/README.md).
#
#   validation/suite.sh PACKAGE LIBC [JOBS]
#
# PACKAGE: coreutils, sed, bash, findutils; since milestone 6 grep, gawk, diffutils, tar, gzip,
# make (the verified tarballs in $VALIDATION, default ~/validation).
# LIBC:
#   glibc     the host's, dynamically linked, as the distribution builds;
#   liella    static, through scripts/liella-cc, as is;
#   liella-w  liella with the first runs' labelled workarounds (validation/README.md,
#             "Workarounds"): --allow-multiple-definition, validation/shim/missing.c, and
#             gnulib's regex instead of Liella's for coreutils and findutils
#             (--with-included-regex; sed keeps Liella's);
#   liella-t1 the second runs (milestone 3): only --allow-multiple-definition (T1, until libc.a
#             has weak definitions) and the libm stand-in of validation/shim/missing.c (T3);
#   liella-t1r liella-t1 with gnulib's regex for coreutils and findutils (to see past T5, the
#             regex fix of the optimization thread).
#   liella-m  the fourth runs (milestone 4, libc.a overridable since D-X.14): as is, except the
#             modf stand-in of validation/shim/missing.c for findutils (T3; results provisional);
#   liella-mr liella-m with gnulib's regex for coreutils and findutils (diagnosis of T5/T6 only);
#             since milestone 6 also for grep, diffutils and tar;
#   glibc-noload, liella-noload  make only: --disable-load under both libraries (T4, T27);
#   glibc-sysregex  glibc with --without-included-regex for coreutils, sed and findutils: their
#             configure rejects glibc's re_compile_pattern and links gnulib's regex, which the
#             corpus harvest cannot see (validation item C); the stock glibc build stays the
#             baseline, this one exists for the harvest.
# HARVEST_SO: when set, `make check` runs with this LD_PRELOAD library
# (validation/harvest/harvest.c) logging into $VALIDATION/harvest/LIBC-PACKAGE.
# The tree is unpacked fresh into $VALIDATION/build/LIBC/PACKAGE; the logs go to
# $VALIDATION/results/LIBC-PACKAGE.*: configure, build and check output, the check's wall time
# and the machine's load before and after (uptime), the configure results gnulib's replacements
# depend on (cache variables, and REPLACE_*/HAVE_* substitutions), and the tests a watchdog had
# to stop (.hung: a package program running longer than $HUNG_SECONDS, default 600).
#
# Clean-room rule: no source under the packages' lib/, gnulib/ or gnulib-tests/ directories is
# read by this script or by anyone triaging; the test harness's own summary lines on standard
# output name the tests, and since ruling R4 the gnulib-tests output files (*.log, *.trs) may be
# read to name a failing check (validation/README.md, "Clean room").
set -u
pkg=$1
libc=$2
jobs=${3:-6}
V=${VALIDATION:-$HOME/validation}
repo=$(cd "$(dirname "$0")/.." && pwd)
case $pkg in
    coreutils) tarball=$V/coreutils-9.12.tar.xz; dir=coreutils-9.12; opts="--without-selinux" ;;
    sed) tarball=$V/sed-4.10.tar.xz; dir=sed-4.10; opts="" ;;
    bash) tarball=$V/bash-5.3.tar.gz; dir=bash-5.3; opts="--without-bash-malloc" ;;
    # Not one of the three suites: built for find in validation/traversal.sh (its suite runs too).
    findutils) tarball=$V/findutils-4.11.0.tar.xz; dir=findutils-4.11.0; opts="--without-selinux" ;;
    # Milestone 6 (validation/README.md): six more packages, stock configure.
    grep) tarball=$V/grep-3.12.tar.xz; dir=grep-3.12; opts="" ;;
    gawk) tarball=$V/gawk-5.4.1.tar.xz; dir=gawk-5.4.1; opts="" ;;
    diffutils) tarball=$V/diffutils-3.12.tar.xz; dir=diffutils-3.12; opts="" ;;
    tar) tarball=$V/tar-1.35.tar.xz; dir=tar-1.35; opts="" ;;
    gzip) tarball=$V/gzip-1.15.tar.xz; dir=gzip-1.15; opts="" ;;
    make) tarball=$V/make-4.4.1.tar.gz; dir=make-4.4.1; opts="--without-guile" ;;
    *) echo "unknown package $pkg" >&2; exit 2 ;;
esac
case $libc in
    glibc) cc=gcc ;;
    # make enables its `load` directive whenever <dlfcn.h> declares dlopen, and a static
    # program has no dlopen (T4, phase 14): the -noload pair builds make with --disable-load
    # under both libraries, labelled, so its suite runs at all.
    glibc-noload) cc=gcc; opts="$opts --disable-load" ;;
    liella-noload) cc=$repo/scripts/liella-cc; opts="$opts --disable-load" ;;
    glibc-sysregex)
        cc=gcc
        case $pkg in coreutils | sed | findutils | grep | diffutils | tar) opts="$opts --without-included-regex" ;; esac ;;
    # As is: no workaround, no stand-in; since V-5 (PRE14-VALIDATION-RERUN) -lm links Liella's
    # libm.a (scripts/liella-cc), so gawk and find need no shim.
    liella) cc=$repo/scripts/liella-cc ;;
    liella-w)
        cc=$repo/scripts/liella-cc
        mkdir -p "$V/shim"
        gcc -O1 -c -o "$V/shim/missing.o" "$repo/validation/shim/missing.c" || exit 1
        export LIELLA_CC_EXTRA="-Wl,--allow-multiple-definition $V/shim/missing.o"
        case $pkg in coreutils | findutils) opts="$opts --with-included-regex" ;; esac ;;
    liella-t1 | liella-t1r)
        cc=$repo/scripts/liella-cc
        mkdir -p "$V/shim"
        gcc -O1 -c -o "$V/shim/missing.o" "$repo/validation/shim/missing.c" || exit 1
        export LIELLA_CC_EXTRA="-Wl,--allow-multiple-definition $V/shim/missing.o"
        if [ "$libc" = liella-t1r ]; then
            case $pkg in coreutils | findutils) opts="$opts --with-included-regex" ;; esac
        fi ;;
    liella-m | liella-mr)
        cc=$repo/scripts/liella-cc
        # The milestone-4..6 builds used maths stand-ins (findutils: modf of shim/missing.c;
        # gawk: shim/libm.c). Since GAWK-LM-CHECK-2 -lm links Liella's libm.a and modf is in
        # libc.a (T23), so no stand-in is linked any more.
        if [ "$libc" = liella-mr ]; then
            case $pkg in coreutils | findutils | grep | diffutils | tar) opts="$opts --with-included-regex" ;; esac
        fi ;;
    *) echo "unknown C library $libc" >&2; exit 2 ;;
esac
# A harvest run (HARVEST_SO) keeps its own tree and results, apart from the plain baseline.
tag=$libc${HARVEST_SO:+-harvest}
out=$V/results/$tag-$pkg
mkdir -p "$V/results" "$V/build/$tag"
rm -rf "$V/build/$tag/$pkg"
mkdir -p "$V/build/$tag/$pkg"
cd "$V/build/$tag/$pkg" || exit 1
tar xf "$tarball" || exit 1
cd "$dir" || exit 1
tree=$(pwd)
if [ "$pkg" = bash ]; then
    # The official patches (signed; verified with the tarball).
    for p in "$V"/patches/bash53-0??; do
        patch -s -p0 < "$p" || { echo "patch $p failed" >&2; exit 1; }
    done
fi
stamp() { echo "$1 $(date -u +%FT%TZ) load: $(uptime | sed 's/.*load average: //')"; }
# Stops a program of the package's build tree that runs longer than the limit (a hung test).
watchdog() {
    : > "$out.hung"
    while sleep 30; do
        ps -eo pid=,etimes= | while read -r pid secs; do
            [ "$secs" -gt "${HUNG_SECONDS:-600}" ] || continue
            exe=$(readlink "/proc/$pid/exe" 2>/dev/null) || continue
            case $exe in
                "$tree"/*)
                    echo "$(date -u +%T) killed after ${secs}s: $(tr '\0' ' ' < "/proc/$pid/cmdline" | cut -c1-200)" >> "$out.hung"
                    kill "$pid" ;;
            esac
        done
    done
}
{
    # The exact code a run was built from (the branch HEAD and the main commit it contains).
    echo "built-from: ${SUITE_COMMIT:-unrecorded}"
    stamp "configure-start"
    # shellcheck disable=SC2086
    CC=$cc ./configure $opts > "$out.configure.log" 2>&1
    echo "configure rc=$?"
    stamp "build-start"
    t0=$(date +%s.%N)
    make -k -j"$jobs" > "$out.build.log" 2>&1
    echo "build rc=$? seconds=$(echo "$(date +%s.%N) - $t0" | bc)"
    watchdog &
    dog=$!
    stamp "check-start"
    t0=$(date +%s.%N)
    if [ -n "${HARVEST_SO:-}" ]; then
        hdir=$V/harvest/$tag-$pkg
        rm -rf "$hdir" && mkdir -p "$hdir"
        LD_PRELOAD=$HARVEST_SO HARVEST_DIR=$hdir HARVEST_TREE=$tree HARVEST_BUILD=$libc-$pkg make -j"$jobs" -k check > "$out.check.log" 2>&1
    else
        make -j"$jobs" -k check > "$out.check.log" 2>&1
    fi
    echo "check rc=$? seconds=$(echo "$(date +%s.%N) - $t0" | bc)"
    stamp "check-end"
    kill $dog
} > "$out.summary" 2>&1
# What configure decided (gnulib's replacements and the function checks they rest on).
sed -n '/^## Cache variables/,/^## ----.*Output variables/p' config.log | grep -E "^(gl_cv_|ac_cv_func_|ac_cv_have_decl_|ac_cv_header_)" | sort > "$out.cache" 2>/dev/null
sed -n '/^## Output variables/,/^## ----.*File substitutions/p' config.log | grep -E "^(REPLACE|HAVE|GNULIB)_[A-Z0-9_]+='" | sort > "$out.replace" 2>/dev/null
# Automake's per-test results as printed on standard output.
grep -E "^(PASS|FAIL|SKIP|XFAIL|XPASS|ERROR): " "$out.check.log" > "$out.results" 2>/dev/null
# tar (autotest), make (its Perl driver) and gawk (its own Makefile) print their own formats;
# their per-test lines are converted to the same form, after automake's (their gnulib tests).
case $pkg in
    tar | make | gawk) python3 "$repo/validation/results_other.py" "$pkg" "$out.check.log" >> "$out.results" ;;
esac
cat "$out.summary" "$out.hung"
