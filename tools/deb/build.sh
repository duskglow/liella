#!/bin/sh
# tools/deb/build.sh: build Liella's reference Debian packages (docs/install-packages.md; the layout
# is the pending decision records/pending/RELEASE-PACKAGING-DEB-1.toml).
#
#   tools/deb/build.sh [--out DIR] [--work DIR]
#       From a clean tree: the committed HEAD is exported (git archive) into WORK/src, the products
#       are built there in the release profile with the packages' loader layout
#       (LIELLA_PREFIX=/usr/lib/liella, LIELLA_LIBDIR=usr/lib/x86_64-linux-gnu), and the packages
#       are written to OUT (default target/deb). Uncommitted changes are never packaged.
#   tools/deb/build.sh --products DIR --version V [--out DIR] [--work DIR]
#       Package an existing products directory (built with that layout) as version V; no build.
#
# Packages: liella6 (run time: the loader and the shared libraries) and liella6-dev (link time:
# start files, linker scripts, static archives, the liella-gcc/liella-g++ wrapper). Every file
# is under the sysroot /usr/lib/liella except the wrapper, its manual page and the packages'
# /usr/share/doc directories: nothing of the system's C library is touched (tools/deb/check.sh
# enforces it).
#
# Reproducible: SOURCE_DATE_EPOCH (default: the HEAD commit's time; 1 with --products) is every
# file's time and the time dpkg-deb records; owners are root:root (--root-owner-group); dpkg-deb
# writes members in sorted order; modes are fixed below. Building twice gives identical .debs.
#
# Version: 0.0~snapshotYYYYMMDD+<12-hex commit> (YYYYMMDD: the commit's UTC date), a native
# version that sorts below any 0.0 release.
set -eu
umask 022
export LC_ALL=C TZ=UTC

here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/../.." && pwd)
out=$root/target/deb
work=$root/target/deb-work
products=""
version=""
while [ $# -gt 0 ]; do
    case $1 in
        --out) out=$2; shift 2 ;;
        --work) work=$2; shift 2 ;;
        --products) products=$2; shift 2 ;;
        --version) version=$2; shift 2 ;;
        *) echo "usage: tools/deb/build.sh [--out DIR] [--work DIR] [--products DIR --version V]" >&2; exit 2 ;;
    esac
done

sysroot=/usr/lib/liella
ma=x86_64-linux-gnu
maint=${DEB_MAINTAINER:-Liella project <liella-packaging@invalid>}

if [ -z "$products" ]; then
    [ -z "$version" ] || { echo "build.sh: --version goes with --products" >&2; exit 2; }
    rm -rf "$work/src"
    mkdir -p "$work/src"
    if git -C "$root" rev-parse -q --verify HEAD > /dev/null 2>&1; then
        commit=$(git -C "$root" rev-parse --short=12 HEAD)
        epoch=${SOURCE_DATE_EPOCH:-$(git -C "$root" log -1 --format=%ct HEAD)}
        if [ -n "$(git -C "$root" status --porcelain --untracked-files=no)" ]; then
            echo "build.sh: note: the tree has uncommitted changes; they are not packaged (HEAD $commit is)"
        fi
        git -C "$root" archive --format=tar HEAD | tar -x -C "$work/src"
    else
        # A synced copy without .git (a build host's role directory): the caller names the commit
        # it holds and its time; the files are packaged as they are, target/ left out.
        if [ -z "${LIELLA_DEB_COMMIT:-}" ] || [ -z "${SOURCE_DATE_EPOCH:-}" ]; then
            echo "build.sh: $root is not a git tree: set LIELLA_DEB_COMMIT and SOURCE_DATE_EPOCH" >&2
            exit 2
        fi
        commit=$(printf '%.12s' "$LIELLA_DEB_COMMIT")
        epoch=$SOURCE_DATE_EPOCH
        echo "build.sh: note: $root is not a git tree; packaging its files as commit $commit"
        tar -C "$root" --exclude=./target --exclude=./.git -cf - . | tar -x -C "$work/src"
    fi
    version=0.0~snapshot$(date -u -d "@$epoch" +%Y%m%d)+$commit
    echo "build.sh: building the products of $commit (release profile) in $work/src"
    (cd "$work/src" && LIELLA_PREFIX=$sysroot LIELLA_LIBDIR=usr/lib/$ma LIELLA_FULL_PROFILE=1 SOURCE_DATE_EPOCH=$epoch cargo xtask so)
    products=$work/src/target/liella
else
    [ -n "$version" ] || { echo "build.sh: --products needs --version" >&2; exit 2; }
    epoch=${SOURCE_DATE_EPOCH:-1}
fi
products=$(cd "$products" && pwd)
export SOURCE_DATE_EPOCH="$epoch"

stage=$work/stage
rm -rf "$stage"
rt=$stage/liella6
dev=$stage/liella6-dev

# put MODE SRC DEST: a copy of SRC at DEST (parents made).
put() {
    mkdir -p "$(dirname "$3")"
    cp "$2" "$3"
    chmod "$1" "$3"
}
# put_link TARGET LINK: a symbolic link LINK -> TARGET (parents made).
put_link() {
    mkdir -p "$(dirname "$2")"
    ln -s "$1" "$2"
}
# put_text MODE DEST TEXT: a file holding TEXT and a newline.
put_text() {
    mkdir -p "$(dirname "$2")"
    printf '%s\n' "$3" > "$2"
    chmod "$1" "$2"
}
need() {
    [ -f "$products/$1" ] || { echo "build.sh: $products/$1 is missing (cargo xtask so)" >&2; exit 1; }
}

# --- liella6: the run-time sysroot, laid out as Debian's libc6 lays out / (merged /usr). ---
rlib=$rt$sysroot/usr/lib/$ma
need ld-linux-x86-64.so.2
put 0755 "$products/ld-linux-x86-64.so.2" "$rlib/ld-linux-x86-64.so.2"
for so in libc.so.6 libm.so.6 libpthread.so.0 libdl.so.2 librt.so.1 libutil.so.1 libanl.so.1 libnsl.so.1 libc_malloc_debug.so.0; do
    need "$so"
    put 0644 "$products/$so" "$rlib/$so"
done
# The program interpreter's path, /usr/lib/liella/lib64/ld-linux-x86-64.so.2, as Debian's
# /lib64/ld-linux-x86-64.so.2: lib64 -> usr/lib64, usr/lib64/<loader> -> the multiarch copy.
put_link ../lib/$ma/ld-linux-x86-64.so.2 "$rt$sysroot/usr/lib64/ld-linux-x86-64.so.2"
put_link usr/lib "$rt$sysroot/lib"
put_link usr/lib64 "$rt$sysroot/lib64"

# --- liella6-dev: link time, as Debian's libc6-dev (static archives included, as there). ---
dlib=$dev$sysroot/usr/lib/$ma
need liella-crt1.o
need liella-rcrt1.o
put 0644 "$products/liella-crt1.o" "$dlib/crt1.o"
put 0644 "$products/liella-crt1.o" "$dlib/Scrt1.o"
put 0644 "$products/liella-rcrt1.o" "$dlib/rcrt1.o"
for a in libc.a libm.a libc_nonshared.a libpthread.a libdl.a librt.a libutil.a libanl.a libresolv.a; do
    need "$a"
    put 0644 "$products/$a" "$dlib/$a"
done
# The linker scripts name files inside the sysroot; the wrapper links with --sysroot, so the link
# editor reads them there (the script lies in the sysroot).
put_text 0644 "$dlib/libc.so" "/* GNU ld script: Liella's C library (liella6-dev). Paths are inside the sysroot
   /usr/lib/liella; link with --sysroot=/usr/lib/liella (liella-gcc does). */
OUTPUT_FORMAT(elf64-x86-64)
GROUP ( /lib/$ma/libc.so.6 /usr/lib/$ma/libc_nonshared.a  AS_NEEDED ( /lib64/ld-linux-x86-64.so.2 ) )"
put_link libm.so.6 "$dlib/libm.so"
put 0755 "$here/liella-gcc" "$dev/usr/bin/liella-gcc"
put_link liella-gcc "$dev/usr/bin/liella-g++"
mkdir -p "$dev/usr/share/man/man1"
gzip -9n < "$here/liella-gcc.1" > "$dev/usr/share/man/man1/liella-gcc.1.gz"
put_link liella-gcc.1.gz "$dev/usr/share/man/man1/liella-g++.1.gz"

# --- Documentation of both packages. ---
date_r=$(date -u -d "@$epoch" -R)
for p in liella6 liella6-dev; do
    d=$stage/$p/usr/share/doc/$p
    put 0644 "$here/copyright" "$d/copyright"
    printf 'liella (%s) unstable; urgency=medium\n\n  * Snapshot of Liella (reference packages; docs/install-packages.md).\n\n -- %s  %s\n' \
        "$version" "$maint" "$date_r" | gzip -9n > "$d/changelog.gz"
done

# Strip the shared objects as Debian does (debug sections, .comment, .note); the archives and
# start files keep their symbols (the link needs them) but lose debug sections and .comment.
find "$rt$sysroot" -type f -name '*.so*' -exec strip --strip-unneeded --remove-section=.comment --remove-section=.note {} +
find "$dev$sysroot" -type f \( -name '*.a' -o -name '*.o' \) -exec strip --strip-debug --remove-section=.comment --enable-deterministic-archives {} +

# installed_size DIR: Debian's Installed-Size (KiB): each file's size rounded up, 1 per other entry.
installed_size() {
    find "$1" -path "$1/DEBIAN" -prune -o -mindepth 1 -printf '%y %s\n' |
        awk '$1 == "f" { t += int(($2 + 1023) / 1024); next } { t += 1 } END { print t + 0 }'
}
# control PKG SECTION DEPENDS DESCRIPTION...
control() {
    p=$1; sec=$2; deps=$3; shift 3
    mkdir -p "$stage/$p/DEBIAN"
    {
        echo "Package: $p"
        echo "Source: liella"
        echo "Version: $version"
        echo "Architecture: amd64"
        echo "Maintainer: $maint"
        echo "Installed-Size: $(installed_size "$stage/$p")"
        [ -z "$deps" ] || echo "Depends: $deps"
        [ "$p" != liella6-dev ] || echo "Recommends: g++"
        echo "Section: $sec"
        echo "Priority: optional"
        for line in "$@"; do echo "$line"; done
    } > "$stage/$p/DEBIAN/control"
    (cd "$stage/$p" && find . -path ./DEBIAN -prune -o -type f -printf '%P\n' | sort | xargs -r md5sum) > "$stage/$p/DEBIAN/md5sums"
}
control liella6 libs "" \
    "Description: Liella C library (run time, beside the system's C library)" \
    " Liella is an implementation of the GNU C Library's ABI (glibc 2.39, x86-64)." \
    " This package installs its loader and shared libraries in the sysroot" \
    " /usr/lib/liella. It does not replace or change the system's C library: only" \
    " programs whose interpreter is /usr/lib/liella/lib64/ld-linux-x86-64.so.2" \
    " (built with liella-gcc from liella6-dev) run on Liella."
control liella6-dev libdevel "liella6 (= $version), gcc, libc6-dev, binutils" \
    "Description: Liella C library (development files and compiler wrapper)" \
    " Liella is an implementation of the GNU C Library's ABI (glibc 2.39, x86-64)." \
    " This package installs the start files, linker scripts and static archives in" \
    " the sysroot /usr/lib/liella, and liella-gcc and liella-g++, which build" \
    " programs that run on Liella's loader. Programs compile with the system's C" \
    " headers (from libc6-dev)."

# Fixed modes and times.
find "$stage" -type d -exec chmod 0755 {} +
find "$stage" -path '*/DEBIAN/*' -type f -exec chmod 0644 {} +
find "$stage" -exec touch -h -d "@$epoch" {} +

mkdir -p "$out"
for p in liella6 liella6-dev; do
    deb=$out/${p}_${version}_amd64.deb
    dpkg-deb --root-owner-group -Zxz -z9 --build "$stage/$p" "$deb" >/dev/null
    echo "build.sh: $(sha256sum "$deb" | cut -c1-64)  $deb"
done
