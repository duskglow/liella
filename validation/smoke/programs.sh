# validation/smoke/programs.sh: the programs smoke.sh builds, sourced by it. For each program NAME:
#   NAME_src       the Debian source package (fetched from the trixie suite on deb.debian.org)
#   NAME_kind      how its test result is read: automake (test-suite.log files), rc (one suite, its status),
#                  gawk ("N TESTS FAILED" / "ALL TESTS PASSED"), make ("N Tests in M Categories Failed"),
#                  bash (run-* sections compared with the reference build's), perl (t/TEST summary),
#                  python (regrtest summary)
#   NAME_build     configure and build in the current directory (a fresh copy of the source); CC and CXX are set
#   NAME_test      run the program's own test suite there
#   NAME_skip      (optional) why its test suite does not run here
# Every build runs twice: CC=gcc (the reference: the host's C library) and CC=liella-cc (Liella). The
# configure options turn off optional libraries the host has no headers for, and NLS (gettext catalogues
# are not under test). Autotools files are never regenerated (smoke.sh puts AUTOCONF=true and the like in
# MAKEFLAGS): the host has no autotools; Debian's patches to configure.ac or Makefile.am are not applied by
# a regeneration, which this report says where it matters.
# shellcheck shell=sh disable=SC2034

zlib_src=zlib
zlib_kind=rc
zlib_build() { CFLAGS="-O2 -g" ./configure --static && make -j"$J"; }
zlib_test() { make test; }

xz_utils_src=xz-utils
xz_utils_kind=automake
xz_utils_build() { ./configure --disable-shared --disable-nls --disable-doc && make -j"$J"; }
xz_utils_test() { make -k -j"$J" check; }

# sqlite3: the Tcl test suite needs the Tcl development files (not on the host); the suite that runs without
# them is fuzzcheck over the fuzz databases in test/ (`make fuzztest` also builds sessionfuzz, which needs
# zlib.h, also not on the host). --disable-load-extension: Liella's libc.so.6 does not export dlsym and
# dlclose yet (DL-16b, DL-17), so the extension loader does not link; Debian builds it enabled.
sqlite3_src=sqlite3
sqlite3_kind=rc
sqlite3_build() {
    ./configure --disable-shared --disable-tcl --disable-readline --disable-load-extension &&
        make -j"$J" sqlite3 fuzzcheck
}
sqlite3_test() { ./fuzzcheck test/fuzzdata*.db; }

grep_src=grep
grep_kind=automake
grep_build() { ./configure --disable-nls --disable-perl-regexp && make -j"$J"; }
grep_test() { make -k -j"$J" check; }

sed_src=sed
sed_kind=automake
sed_build() { ./configure --disable-nls --without-selinux --disable-acl && make -j"$J"; }
sed_test() { make -k -j"$J" check; }

diffutils_src=diffutils
diffutils_kind=automake
diffutils_build() { ./configure --disable-nls && make -j"$J"; }
diffutils_test() { make -k -j"$J" check; }

findutils_src=findutils
findutils_kind=automake
findutils_build() { ./configure --disable-nls --without-selinux && make -j"$J"; }
findutils_test() { make -k -j"$J" check; }

coreutils_src=coreutils
coreutils_kind=automake
coreutils_build() {
    ./configure --disable-nls --without-selinux --disable-acl --disable-xattr --disable-libcap --without-openssl \
        --without-libgmp && make -j"$J"
}
coreutils_test() { make -k -j"$J" check; }

# gawk: the interpreter only (SUBDIRS without doc/, whose manual source the Debian package builds from a
# file it generates, and without extension/: loading extensions needs dlsym, DL-16b).
gawk_src=gawk
gawk_kind=gawk
gawk_build() {
    ./configure --disable-nls --without-readline --without-mpfr --disable-extensions &&
        make -j"$J" SUBDIRS="support ."
}
gawk_test() { make check SUBDIRS="support . test"; }

# make-dfsg: Debian removes doc/ (the manual and make.1) and regenerates configure without it; without autotools on the host the
# shipped configure still lists doc/Makefile, so the build gives it an empty one (a harness step, not a
# change to the program).
make_src=make-dfsg
make_kind=make
make_build() {
    [ -f doc/Makefile.in ] || { mkdir -p doc && printf 'all check install clean distclean:\n' > doc/Makefile.in; }
    ./configure --disable-nls --without-guile && make -j"$J" make
}
# The regression suite alone (`check` also wants the manual page, which the dfsg source does not have).
make_test() { make check-local; }

bash_src=bash
bash_kind=bash
bash_build() { ./configure --without-bash-malloc --disable-nls --without-curses && make -j"$J"; }
bash_test() { make tests; }

# perl: miniperl and `make minitest` (the core tests that run without extensions: DynaLoader needs dlsym,
# which Liella's libc.so.6 does not export yet, DL-16b). libswanted keeps the link to libm and the thread
# library (libcrypt, libgdbm and the like would be system libraries found only by a library search).
perl_src=perl
perl_kind=perl
perl_build() {
    sh Configure -des -Dcc="$CC" -Dprefix="$PWD/inst" -Dlibswanted="m pthread" -Dusethreads && make -j"$J" miniperl
}
# `make minitest` reads stdin from /dev/tty, which a job has not got; minitest-notty is the same run without it.
perl_test() { make minitest-notty; }

# python3.13: every extension module built in (MODULE_BUILDTYPE=static) and the dynamic loader disabled
# (ac_cv_func_dlopen=no): the import of extension modules needs dlsym (DL-16b). Modules whose libraries the
# host has no headers for (zlib, ssl, ctypes, ...) are not built on either side. The test run is a sample.
python3_src=python3.13
python3_kind=python
python3_tests="test_os test_posix test_re test_math test_float test_long test_struct test_string test_str
 test_time test_datetime test_strftime test_locale test_threading test_thread test_subprocess test_signal
 test_select test_fcntl test_socket test_tempfile test_shutil test_glob test_fnmatch test_pwd test_grp
 test_resource test_mmap test_errno test_sys test_unicodedata test_codecs test_format test_builtin test_dict"
python3_build() {
    MODULE_BUILDTYPE=static ac_cv_func_dlopen=no ac_cv_lib_dl_dlopen=no ./configure --without-ensurepip \
        --disable-test-modules && make -j"$J"
}
python3_test() {
    # shellcheck disable=SC2086
    ./python -m test -j"$J" --timeout 1200 $python3_tests
}

# cppunit: a C++ library and its own test program (iostream, exceptions, the C++ runtime) built with
# liella-c++; libstdc++ is linked statically (no libstdc++.so.6 to search for).
cppunit_src=cppunit
cppunit_kind=automake
# Only the library and its test program (examples/): the plug-in tester DllPlugInTester links dlsym and
# dlclose, which Liella's libc.so.6 does not export yet (DL-16b, DL-17).
cppunit_build() {
    ./configure --disable-shared --disable-doxygen --disable-html-docs && make -j"$J" -C src/cppunit
}
cppunit_test() { make -k -j"$J" -C examples check; }
