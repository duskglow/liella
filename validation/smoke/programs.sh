# validation/smoke/programs.sh: the programs smoke.sh builds, sourced by it. For each program NAME:
#   NAME_src       the Debian source package (fetched from the trixie suite on deb.debian.org)
#   NAME_kind      how its test result is read: automake (test-suite.log files), rc (one suite, its status),
#                  gawk ("N TESTS FAILED" / "ALL TESTS PASSED"), make ("N Tests in M Categories Failed"),
#                  bash (run-* sections compared with the reference build's)
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
xz_utils_test() { make -j"$J" check; }

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
grep_test() { make -j"$J" check; }

sed_src=sed
sed_kind=automake
sed_build() { ./configure --disable-nls --without-selinux --disable-acl && make -j"$J"; }
sed_test() { make -j"$J" check; }

diffutils_src=diffutils
diffutils_kind=automake
diffutils_build() { ./configure --disable-nls && make -j"$J"; }
diffutils_test() { make -j"$J" check; }

findutils_src=findutils
findutils_kind=automake
findutils_build() { ./configure --disable-nls --without-selinux && make -j"$J"; }
findutils_test() { make -j"$J" check; }

coreutils_src=coreutils
coreutils_kind=automake
coreutils_build() {
    ./configure --disable-nls --without-selinux --disable-acl --disable-xattr --disable-libcap --without-openssl \
        --without-libgmp && make -j"$J"
}
coreutils_test() { make -j"$J" check; }

gawk_src=gawk
gawk_kind=gawk
gawk_build() { ./configure --disable-nls --without-readline --without-mpfr && make -j"$J"; }
gawk_test() { make check; }

make_src=make-dfsg
make_kind=make
make_build() { ./configure --disable-nls --without-guile && make -j"$J"; }
make_test() { make check; }

bash_src=bash
bash_kind=bash
bash_build() { ./configure --without-bash-malloc --disable-nls --without-curses && make -j"$J"; }
bash_test() { make tests; }
